//! Control-flow analysis for function bodies.
//!
//! Answers three questions the scope walk cannot: whether a block always exits,
//! whether the `return` statements in one function agree on producing a value,
//! and whether a condition operand is provably not a `Bool`.
//!
//! The analysis is deliberately syntactic and conservative. A path it cannot
//! prove stays unclassified, and an unknown condition is accepted, so the pass
//! only reports what it can prove wrong.

use aipo_ast::Literal;
use aipo_hir::{HirExpr, HirFunctionDecl, HirStmt};
use aipo_source::SourceSpan;

/// How a statement block ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockFlow {
    /// Control can reach the end of the block normally.
    FallsThrough,
    /// Every path through the block exits it (`return`, `break`, `continue`, `fail`).
    Diverges,
}

/// What a block's `return` statements do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReturnShape {
    /// The block contains no `return`.
    NoReturn,
    /// Every `return` carries a value.
    AllValues,
    /// Every `return` carries no value.
    AllVoid,
    /// Some `return` carries a value and another does not.
    Mixed,
}

/// Result of analysing one function body.
#[derive(Debug, Clone)]
pub(crate) struct FlowReport {
    /// Aggregated return shape of the whole body.
    pub returns: ReturnShape,
    /// `true` when the body ends without a value on at least one path but has a
    /// value-returning path, meaning some path falls out of the function.
    pub missing_value_path: bool,
}

/// Analyses `function`'s body for return consistency.
pub(crate) fn analyze_function(function: &HirFunctionDecl) -> FlowReport {
    let mut collector = Collector::default();
    let flow = collector.visit_block(&function.body);

    // Reaching the end of the body after a value-returning path means some path
    // produces `none` instead of a value.
    let falls_off = flow == BlockFlow::FallsThrough;
    collector.value_path_falls_off = falls_off && collector.saw_value_return;

    collector.finish()
}

/// Default-constructible state threaded through the walk.
#[derive(Debug, Default)]
struct Collector {
    /// Whether any `return` carried a value.
    saw_value_return: bool,
    /// Whether any `return` carried no value.
    saw_void_return: bool,
    /// Span of the first value-returning `return`, for the mismatch report.
    value_return_span: Option<SourceSpan>,
    /// Span of the first void `return`, for the mismatch report.
    void_return_span: Option<SourceSpan>,
    /// Whether a value-returning path can reach the end of the body.
    value_path_falls_off: bool,
}

impl Collector {
    /// Folds the collected observations into a report.
    fn finish(self) -> FlowReport {
        let returns = match (self.saw_value_return, self.saw_void_return) {
            (true, true) => ReturnShape::Mixed,
            (true, false) => ReturnShape::AllValues,
            (false, true) => ReturnShape::AllVoid,
            (false, false) => ReturnShape::NoReturn,
        };
        FlowReport {
            returns,
            missing_value_path: self.value_path_falls_off,
        }
    }

    /// Records a `return` and returns whether the enclosing block diverges.
    fn record_return(&mut self, value: Option<&HirExpr>, span: SourceSpan) {
        match value {
            Some(_) => {
                self.saw_value_return = true;
                if self.value_return_span.is_none() {
                    self.value_return_span = Some(span);
                }
            }
            None => {
                self.saw_void_return = true;
                if self.void_return_span.is_none() {
                    self.void_return_span = Some(span);
                }
            }
        }
    }

    /// Visits a block of statements and reports whether it always exits.
    fn visit_block(&mut self, stmts: &[HirStmt]) -> BlockFlow {
        for stmt in stmts {
            if self.visit_stmt(stmt) == BlockFlow::Diverges {
                return BlockFlow::Diverges;
            }
        }
        BlockFlow::FallsThrough
    }

    /// Visits a statement in a position where a fall-through path exists.
    ///
    /// Used for `if` branches and loop bodies, where falling off the end of the
    /// inner block returns to the enclosing statement.
    fn visit_stmt(&mut self, stmt: &HirStmt) -> BlockFlow {
        match stmt {
            HirStmt::Return(value, span) => {
                self.record_return(value.as_ref(), *span);
                BlockFlow::Diverges
            }
            HirStmt::Break(_) | HirStmt::Continue(_) | HirStmt::Fail(_, _) => BlockFlow::Diverges,
            HirStmt::If(if_stmt) => {
                let then_flow = self.visit_block(&if_stmt.then_branch);
                for (condition, body) in &if_stmt.elif_branches {
                    self.visit_expr(condition);
                    self.visit_block(body);
                }
                // A missing `else` is an implicit empty arm, so the condition being
                // false falls through to the statement after the `if`.
                let else_flow = if_stmt
                    .else_branch
                    .as_ref()
                    .map_or(BlockFlow::FallsThrough, |body| self.visit_block(body));

                // The `if` itself diverges only when every arm does.
                if then_flow == BlockFlow::Diverges && else_flow == BlockFlow::Diverges {
                    BlockFlow::Diverges
                } else {
                    BlockFlow::FallsThrough
                }
            }
            HirStmt::Match(match_stmt) => {
                self.visit_expr(&match_stmt.target);
                let mut all_diverged = !match_stmt.when_arms.is_empty();
                for arm in &match_stmt.when_arms {
                    for pattern in &arm.patterns {
                        if let aipo_hir::HirMatchPattern::Value(expr) = pattern {
                            self.visit_expr(expr);
                        }
                    }
                    if let Some(guard) = &arm.guard {
                        self.visit_expr(guard);
                    }
                    if self.visit_block(&arm.body) != BlockFlow::Diverges {
                        all_diverged = false;
                    }
                }
                if let Some(else_body) = &match_stmt.else_arm {
                    if self.visit_block(else_body) != BlockFlow::Diverges {
                        all_diverged = false;
                    }
                } else {
                    all_diverged = false;
                }
                if all_diverged {
                    BlockFlow::Diverges
                } else {
                    BlockFlow::FallsThrough
                }
            }
            HirStmt::While(condition, body, _) => {
                self.visit_expr(condition);
                self.visit_loop_body(body);
                BlockFlow::FallsThrough
            }
            HirStmt::Loop(body, _) => {
                self.visit_loop_body(body);
                BlockFlow::Diverges
            }
            HirStmt::Repeat(count, _, body, _) => {
                self.visit_expr(count);
                self.visit_loop_body(body);
                BlockFlow::FallsThrough
            }
            HirStmt::Each(_, iterable, body, _) => {
                self.visit_expr(iterable);
                self.visit_loop_body(body);
                BlockFlow::FallsThrough
            }
            HirStmt::Attempt(attempt) => {
                let body_flow = self.visit_block(&attempt.body);
                let handler_flow = self.visit_block(&attempt.handler);
                // Reaching the end of the body falls through to the statement after
                // the `attempt`; only a body *and* a handler that both diverge make
                // the whole statement diverge, exactly like an `if`/`else`.
                if body_flow == BlockFlow::Diverges && handler_flow == BlockFlow::Diverges {
                    BlockFlow::Diverges
                } else {
                    BlockFlow::FallsThrough
                }
            }
            HirStmt::AwaitDo(body, _) => self.visit_block(body),
            HirStmt::FnDecl(declaration) => {
                // A local function body belongs to its own return contract.
                let _ = declaration;
                BlockFlow::FallsThrough
            }
            HirStmt::Let(_, expr, _) | HirStmt::Var(_, expr, _) => {
                self.visit_expr(expr);
                BlockFlow::FallsThrough
            }
            HirStmt::Assign(target, expr, _) => {
                self.visit_expr(target);
                self.visit_expr(expr);
                BlockFlow::FallsThrough
            }
            HirStmt::CompoundAssign(_, target, expr, _) => {
                self.visit_expr(target);
                self.visit_expr(expr);
                BlockFlow::FallsThrough
            }
            HirStmt::Expr(expr) => {
                self.visit_expr(expr);
                BlockFlow::FallsThrough
            }
        }
    }

    /// Visits a loop body, where `break` and `continue` are locally handled.
    ///
    /// `break`/`continue` are neutralized by rewriting them to plain statements
    /// during traversal, which this does by not recursing into the constructs
    /// that could re-target them.
    fn visit_loop_body(&mut self, body: &[HirStmt]) {
        let mut inner = Collector {
            saw_value_return: self.saw_value_return,
            saw_void_return: self.saw_void_return,
            value_return_span: self.value_return_span,
            void_return_span: self.void_return_span,
            value_path_falls_off: self.value_path_falls_off,
        };
        inner.visit_block(body);
        self.saw_value_return = inner.saw_value_return;
        self.saw_void_return = inner.saw_void_return;
        self.value_return_span = inner.value_return_span;
        self.void_return_span = inner.void_return_span;
        self.value_path_falls_off = inner.value_path_falls_off;
    }

    /// Visits an expression, recording value-returning paths for falls-off checks.
    fn visit_expr(&mut self, expr: &HirExpr) {
        match expr {
            HirExpr::If(_, then_expr, else_expr, _) => {
                self.visit_expr(then_expr);
                self.visit_expr(else_expr);
            }
            HirExpr::Binary(_, lhs, rhs, _) => {
                self.visit_expr(lhs);
                self.visit_expr(rhs);
            }
            HirExpr::Unary(_, inner, _) => self.visit_expr(inner),
            HirExpr::Call(callee, args, _) => {
                self.visit_expr(callee);
                for arg in args {
                    self.visit_expr(&arg.value);
                }
            }
            HirExpr::Dot(target, _, _) | HirExpr::QuestionDot(target, _, _) => {
                self.visit_expr(target);
            }
            HirExpr::Index(target, index, _) => {
                self.visit_expr(target);
                self.visit_expr(index);
            }
            HirExpr::List(items, _) => {
                for item in items {
                    self.visit_expr(item);
                }
            }
            HirExpr::Dict(entries, _) => {
                for (key, value) in entries {
                    self.visit_expr(key);
                    self.visit_expr(value);
                }
            }
            HirExpr::Construct(_, fields, _) => {
                for (_, value) in fields {
                    self.visit_expr(value);
                }
            }
            HirExpr::With(base, updates, _) => {
                self.visit_expr(base);
                for (_, value) in updates {
                    self.visit_expr(value);
                }
            }
            HirExpr::OrElse(left, right, _) => {
                self.visit_expr(left);
                self.visit_expr(right);
            }
            HirExpr::Await(inner, _) | HirExpr::Try(inner, _) => self.visit_expr(inner),
            HirExpr::Fn(declaration) => {
                let _ = declaration;
            }
            HirExpr::Literal(..) | HirExpr::Identifier(..) => {}
        }
    }
}

/// Why a condition operand is not usable as a `Bool`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConditionProblem {
    /// A literal that is never a `Bool`.
    Literal(Literal),
}

/// Returns `Some` when a condition operand is provably not a `Bool`.
///
/// Only literals are decidable: `none` and any non-boolean literal are rejected,
/// while every other expression is accepted because its type is not statically
/// known.
pub(crate) fn non_bool_condition(expr: &HirExpr) -> Option<ConditionProblem> {
    match expr {
        HirExpr::Literal(literal, _) => match literal {
            Literal::Bool(_) => None,
            other => Some(ConditionProblem::Literal(other.clone())),
        },
        HirExpr::Unary(aipo_ast::UnaryOp::Not, inner, _) => non_bool_condition(inner),
        _ => None,
    }
}
