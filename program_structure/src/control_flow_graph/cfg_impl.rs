use log::{debug, trace};
use std::collections::HashSet;
use std::convert::TryFrom;
use std::convert::TryInto;

use crate::ast;
use crate::ast::Definition;
use crate::error_definition::ReportCollection;
use crate::function_data::FunctionData;

use crate::ir;
use crate::ir::declaration_map::DeclarationMap;
use crate::ir::{IREnvironment, TryIntoIR};
use crate::nonempty_vec::NonEmptyVec;
use crate::ssa::dominator_tree::DominatorTree;
use crate::template_data::TemplateData;

use super::basic_block::BasicBlock;
use super::cfg::Cfg;
use super::errors::{CFGError, CFGResult};
use super::unique_vars::ensure_unique_variables;

// Environment used to track variable types.
type BasicBlockVec = NonEmptyVec<BasicBlock>;

type Index = usize;
type IndexSet = HashSet<Index>;

impl TryFrom<&TemplateData> for (Cfg, ReportCollection) {
    type Error = CFGError;

    fn try_from(template: &TemplateData) -> CFGResult<(Cfg, ReportCollection)> {
        let name = template.get_name().to_string();
        let param_data = template.into();

        // Ensure that variable names are globally unique before converting to basic blocks.
        let mut template_body = template.get_body().clone();
        let reports = ensure_unique_variables(&mut template_body, &param_data)?;

        // Convert template AST to CFG and compute dominator tree.
        debug!("building CFG for `{name}`");
        let mut env = IREnvironment::from(&param_data);
        let basic_blocks = build_basic_blocks(&template_body, &mut env)?;
        let dominator_tree = DominatorTree::new(&basic_blocks);
        let declarations = DeclarationMap::from(&env);
        Ok((
            Cfg::new(name, param_data, declarations, basic_blocks, dominator_tree),
            reports,
        ))
    }
}

impl TryFrom<&FunctionData> for (Cfg, ReportCollection) {
    type Error = CFGError;

    fn try_from(function: &FunctionData) -> CFGResult<(Cfg, ReportCollection)> {
        let name = function.get_name().to_string();
        let param_data = function.into();

        // Ensure that variable names are globally unique before converting to basic blocks.
        let mut function_body = function.get_body().clone();
        let reports = ensure_unique_variables(&mut function_body, &param_data)?;

        // Convert function AST to CFG and compute dominator tree.
        debug!("building CFG for `{name}`");
        let mut env = IREnvironment::from(&param_data);
        let basic_blocks = build_basic_blocks(&function_body, &mut env)?;
        let dominator_tree = DominatorTree::new(&basic_blocks);
        let declarations = DeclarationMap::from(&env);
        Ok((
            Cfg::new(name, param_data, declarations, basic_blocks, dominator_tree),
            reports,
        ))
    }
}

impl TryFrom<&Definition> for (Cfg, ReportCollection) {
    type Error = CFGError;

    fn try_from(definition: &Definition) -> CFGResult<(Cfg, ReportCollection)> {
        match definition {
            Definition::Function { name, body, .. } | Definition::Template { name, body, .. } => {
                let param_data = definition.into();

                // Ensure that variable names are globally unique before converting to basic blocks.
                let mut body = body.clone();
                let reports = ensure_unique_variables(&mut body, &param_data)?;

                // Convert AST to CFG and compute dominator tree.
                debug!("building CFG for `{name}`");
                let mut env = IREnvironment::from(&param_data);
                let basic_blocks = build_basic_blocks(&body, &mut env)?;
                let dominator_tree = DominatorTree::new(&basic_blocks);
                let declarations = DeclarationMap::from(&env);
                Ok((
                    Cfg::new(
                        name.to_string(),
                        param_data,
                        declarations,
                        basic_blocks,
                        dominator_tree,
                    ),
                    reports,
                ))
            }
        }
    }
}

impl TryFrom<Definition> for (Cfg, ReportCollection) {
    type Error = CFGError;

    fn try_from(definition: Definition) -> CFGResult<(Cfg, ReportCollection)> {
        (&definition).try_into()
    }
}

/// This function generates a vector of basic blocks containing `ir::Statement`s
/// from a function or template body. The vector is guaranteed to be non-empty,
/// and the first block (with index 0) will always be the entry block.
pub(crate) fn build_basic_blocks(
    body: &ast::Statement,
    env: &mut IREnvironment,
) -> CFGResult<Vec<BasicBlock>> {
    assert!(matches!(body, ast::Statement::Block { .. }));

    let mut basic_blocks =
        BasicBlockVec::new(BasicBlock::new(Index::default(), body.get_meta().into()));
    visit_statement(body, env, &mut basic_blocks)?;
    Ok(basic_blocks.into())
}

/// Update the CFG with the current statement. This implementation assumes that
/// all control-flow statement bodies are wrapped by a `Block` statement. Blocks
/// are finalized and the current block (i.e. last block) is updated when:
///
///   2. The current statement is a `While` statement. An `IfThenElse` statement
///      is added to the current block. The successors of the if-statement will be
///      the while-statement body and the while-statement successor (if any).
///   3. The current statement is an `IfThenElse` statement. The current statement
///      is added to the current block. The successors of the if-statement will
///      be the if-case body and else-case body (if any).
///
/// The function returns the predecessors of the next block in the CFG.
fn visit_statement(
    stmt: &ast::Statement,
    env: &mut IREnvironment,
    basic_blocks: &mut BasicBlockVec,
) -> CFGResult<IndexSet> {
    let current_index = basic_blocks.last().get_index();

    use crate::ast::Statement::*;
    match stmt {
        InitializationBlock {
            initializations: stmts,
            ..
        } => {
            // Add each statement in the initialization block to the current
            // block. Since initialization blocks only contain declarations and
            // substitutions, we do not need to track predecessors here.
            trace!("entering initialization block statement");
            for stmt in stmts {
                assert!(visit_statement(stmt, env, basic_blocks)?.is_empty());
            }
            trace!("leaving initialization block statement");
            Ok(HashSet::new())
        }
        Block { stmts, .. } => {
            // Add each statement in the basic block to the current block. If a
            // call to `visit_statement` completes basic block and returns a set
            // of predecessors for the next block, we create a new block before
            // continuing.
            trace!("entering block statement");
            env.add_variable_block();

            let mut pred_set = IndexSet::new();
            for stmt in stmts {
                if !pred_set.is_empty() {
                    let meta = stmt.get_meta().into();
                    complete_basic_block(basic_blocks, &pred_set, meta);
                }
                pred_set = visit_statement(stmt, env, basic_blocks)?;
            }
            trace!("leaving block statement (predecessors: {:?})", pred_set);
            env.remove_variable_block();

            // If the last statement of the block is a control-flow statement,
            // `pred_set` will be non-empty. Otherwise it will be empty.
            Ok(pred_set)
        }
        While {
            meta,
            cond,
            stmt: while_body,
            ..
        } => {
            let pred_set = HashSet::from([current_index]);
            complete_basic_block(basic_blocks, &pred_set, meta.into());

            // While statements are translated into a loop head with a single
            // if-statement, and a loop body containing the while-statement
            // body. The index of the loop header will be `current_index + 1`,
            // and the index of the loop body will be `current_index + 2`.
            trace!("appending statement `if {cond}` to basic block {current_index}");
            basic_blocks
                .last_mut()
                .append_statement(ir::Statement::IfThenElse {
                    meta: meta.into(),
                    cond: cond.try_into_ir(env)?,
                    if_true: current_index + 2,
                    if_false: None, // May be updated later.
                });
            let header_index = current_index + 1;

            // Visit the while-statement body.
            let meta = while_body.get_meta().into();
            let pred_set = HashSet::from([header_index]);
            complete_basic_block(basic_blocks, &pred_set, meta);

            trace!("visiting while body");
            let mut pred_set = visit_statement(while_body, env, basic_blocks)?;
            // The returned predecessor set will be empty if the last statement
            // of the body is not a conditional. In this case we need to add the
            // last block of the body to complete the corresponding block.
            if pred_set.is_empty() {
                pred_set.insert(basic_blocks.last().get_index());
            }
            // The loop header is the successor of all blocks in `pred_set`.
            trace!(
                "adding predecessors {:?} to loop header {header_index}",
                pred_set
            );
            for i in pred_set {
                basic_blocks[i].add_successor(header_index);
                basic_blocks[header_index].add_predecessor(i);
            }

            // The next block (if any) will be the false branch and a successor
            // of the loop header.
            Ok(HashSet::from([header_index]))
        }
        IfThenElse {
            meta,
            cond,
            if_case,
            else_case,
            ..
        } => {
            trace!("appending statement `if {cond}` to basic block {current_index}");
            basic_blocks
                .last_mut()
                .append_statement(ir::Statement::IfThenElse {
                    meta: meta.into(),
                    cond: cond.try_into_ir(env)?,
                    if_true: current_index + 1,
                    if_false: None, // May be updated later.
                });

            // Visit the if-case body.
            let meta = if_case.get_meta().into();
            let pred_set = HashSet::from([current_index]);
            complete_basic_block(basic_blocks, &pred_set, meta);

            trace!("visiting true if-statement branch");
            let mut if_pred_set = visit_statement(if_case, env, basic_blocks)?;
            // The returned predecessor set will be empty if the last statement
            // of the body is not a conditional. In this case we need to add the
            // last block of the body to complete the corresponding block.
            if if_pred_set.is_empty() {
                if_pred_set.insert(basic_blocks.last().get_index());
            }

            // Visit the else-case body.
            if let Some(else_case) = else_case {
                trace!("visiting false if-statement branch");
                let meta = else_case.get_meta().into();
                let pred_set = HashSet::from([current_index]);
                complete_basic_block(basic_blocks, &pred_set, meta);

                let mut else_pred_set = visit_statement(else_case, env, basic_blocks)?;
                // The returned predecessor set will be empty if the last statement
                // of the body is not a conditional. In this case we need to add the
                // last block of the body to complete the corresponding block.
                if else_pred_set.is_empty() {
                    else_pred_set.insert(basic_blocks.last().get_index());
                }
                Ok(if_pred_set.union(&else_pred_set).cloned().collect())
            } else {
                if_pred_set.insert(current_index);
                Ok(if_pred_set)
            }
        }
        Declaration {
            meta,
            name,
            xtype,
            dimensions,
            ..
        } => {
            // Declarations are tracked by the CFG header, so we simply add new declarations to the environment.
            let declaration = (
                meta.clone(),
                name.to_string(),
                xtype.clone(),
                dimensions.clone(),
            )
                .try_into_ir(env)?;
            env.add_declaration(&name, declaration);
            Ok(HashSet::new())
        }
        _ => {
            trace!("appending `{stmt}` to basic block {current_index}");
            basic_blocks
                .last_mut()
                .append_statement(stmt.try_into_ir(env)?);
            Ok(HashSet::new())
        }
    }
}

/// Complete the current (i.e. last) basic block and add a new basic block to
/// the vector with the given `meta`, and `pred_set` as predecessors. Update all
/// predecessors adding the new block as a successor.
///
/// If the final statement of the predecessor block is a control-flow statement,
/// and the new block is not the true branch target of the statement, the new
/// block is added as the false branch target.
fn complete_basic_block(basic_blocks: &mut BasicBlockVec, pred_set: &IndexSet, meta: ir::Meta) {
    use ir::Statement::*;
    trace!("finalizing basic block {}", basic_blocks.last().get_index());
    let j = basic_blocks.len();
    basic_blocks.push(BasicBlock::new(j, meta));
    for i in pred_set {
        basic_blocks[i].add_successor(j);
        basic_blocks[j].add_predecessor(*i);

        // If the final statement `S` of block `i` is a control flow statement,
        // and `j` is not the true branch of `S`, update the false branch of `S`
        // to `j`.
        if let Some(IfThenElse {
            cond,
            if_true,
            if_false,
            ..
        }) = basic_blocks[i].get_statements_mut().last_mut()
        {
            if j != *if_true && if_false.is_none() {
                trace!("updating false branch target of `if {cond}`");
                *if_false = Some(j)
            }
        }
    }
}
