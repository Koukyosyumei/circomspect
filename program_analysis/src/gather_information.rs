use std::collections::HashSet;

use program_structure::abstract_syntax_tree::ast::{
    Access, AssignOp, Expression, ExpressionInfixOpcode, ExpressionPrefixOpcode, SignalType,
    Statement, VariableType,
};
use program_structure::ast::Meta;
use program_structure::program_archive::{ProgramArchive};

pub fn gather_templates_expression(
    expr: &Expression,
    result: &mut HashSet<String>,
    program_archive: &ProgramArchive,
) {
    match expr {
        Expression::Call { id, .. } => {
            result.insert(id.to_string());
            if let Some(template) = program_archive.templates.get(id) {
                gather_templates_statement(template.get_body(), result, program_archive);
            } else if let Some(func) = program_archive.functions.get(id) {
                gather_templates_statement(func.get_body(), result, program_archive);
            }
        }
        Expression::AnonymousComponent { id, .. } => {
            result.insert(id.to_string());
        }
        Expression::InfixOp { lhe, rhe, .. } => {
            gather_templates_expression(lhe, result, program_archive);
            gather_templates_expression(rhe, result, program_archive);
        }
        Expression::PrefixOp { rhe, .. } => {
            gather_templates_expression(rhe, result, program_archive);
        }
        Expression::ParallelOp { rhe, .. } => {
            gather_templates_expression(rhe, result, program_archive);
        }
        _ => {}
    }
}

pub fn gather_templates_statement(
    stmt: &Statement,
    result: &mut HashSet<String>,
    program_archive: &ProgramArchive,
) {
    match stmt {
        Statement::IfThenElse { meta, cond, if_case, else_case } => {
            gather_templates_expression(cond, result, program_archive);
            gather_templates_statement(if_case, result, program_archive);
            if let Some(ecase) = else_case {
                gather_templates_statement(ecase, result, program_archive);
            }
        }
        Statement::While { meta, cond, stmt } => {
            gather_templates_expression(cond, result, program_archive);
            gather_templates_statement(stmt, result, program_archive);
        }
        Statement::Return { meta, value } => {
            gather_templates_expression(value, result, program_archive);
        }
        Statement::InitializationBlock { meta, xtype, initializations } => {
            for ini in initializations {
                gather_templates_statement(ini, result, program_archive);
            }
        }
        Statement::Declaration { meta, xtype, name, dimensions, is_constant } => {}
        Statement::Substitution { meta, var, access, op, rhe } => {
            gather_templates_expression(rhe, result, program_archive);
        }
        Statement::MultiSubstitution { meta, lhe, op, rhe } => {
            gather_templates_expression(rhe, result, program_archive);
        }
        Statement::ConstraintEquality { meta, lhe, rhe } => {
            gather_templates_expression(lhe, result, program_archive);
            gather_templates_expression(rhe, result, program_archive);
        }
        Statement::LogCall { meta, args } => {}
        Statement::Block { meta, stmts } => {
            for stmt in stmts {
                gather_templates_statement(stmt, result, program_archive);
            }
        }
        Statement::Assert { meta, arg } => {
            gather_templates_expression(arg, result, program_archive);
        }
    }
}
