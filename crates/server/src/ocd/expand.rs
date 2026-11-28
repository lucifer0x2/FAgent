//! Turns parsed lines into operations in source order: nesting gives `parentId`, nodes
//! (`node`) get their IDs and written defaults, and directives their operations. Each
//! operation keeps the position of the statement that produced it.
use super::auto_id::Siblings;
use super::directives;
use super::error::{OcdError, Result};
use super::node::{self, Batch, Open};
use super::parser::{DirectiveStmt, Part, Stmt};
use super::{value, Compiled, MAX_OPERATIONS};

fn close(open: Open) -> Result<()> {
    match open.within {
        // `at` is the name after `@`; point at the `@`.
        Some(at) if open.children == 0 => Err(OcdError::new(
            at.line,
            at.col - 1,
            "@in needs indented child nodes",
        )),
        _ => Ok(()),
    }
}

pub fn operations(statements: Vec<(usize, Stmt)>) -> Result<Compiled> {
    let mut out = Compiled::default();
    let mut stack: Vec<Open> = Vec::new();
    let mut batch = Batch::default();
    // `None` before the first line; then the previous directive's name, if it was one
    // that takes no children.
    let mut previous: Option<Option<String>> = None;
    for (depth, statement) in statements {
        let at = match &statement {
            Stmt::Node(node) => &node.head,
            Stmt::Directive(directive) => &directive.name,
        };
        let (line, column) = (at.line, depth * 2 + 1);
        if depth > stack.len() {
            let message = match &previous {
                None => "The first line must not be indented".to_owned(),
                Some(Some(name)) => {
                    format!(
                        "Indented under @{name}, which takes no children; only nodes and @in do"
                    )
                }
                Some(None) => "Indented deeper than one level below the line above".to_owned(),
            };
            return Err(OcdError::new(line, column, message));
        }
        for open in stack.drain(depth..) {
            close(open)?;
        }
        no_null(&statement)?;
        previous = Some(match &statement {
            Stmt::Directive(d) if d.name.text != "in" => Some(d.name.text.clone()),
            _ => None,
        });
        let ops = match statement {
            Stmt::Directive(d) if d.name.text == "in" => {
                stack.push(within(d)?);
                continue;
            }
            Stmt::Directive(d) => directives::operations(d)?,
            Stmt::Node(statement) => vec![node::create(statement, &mut stack, &mut batch)?],
        };
        for op in ops {
            out.operations.push(op);
            out.positions.push((line, column));
        }
        if out.operations.len() > MAX_OPERATIONS {
            return Err(OcdError::new(
                line,
                column,
                format!("Source compiles to more than {MAX_OPERATIONS} operations; split it into batches"),
            ));
        }
    }
    for open in stack {
        close(open)?;
    }
    out.auto_ids = batch.auto_ids;
    Ok(out)
}

/// A bare `null` removes a prop, which only `@set` does; anywhere else it is a mistake for
/// leaving the key out or for the text "null".
fn no_null(statement: &Stmt) -> Result<()> {
    let (parts, values) = match statement {
        Stmt::Node(node) => (&node.parts, &[][..]),
        Stmt::Directive(d) if d.name.text == "set" => return Ok(()),
        // A token's default value is its second positional argument.
        Stmt::Directive(d) if d.name.text == "token" => (&d.parts, &d.positional[1..]),
        Stmt::Directive(d) => (&d.parts, &[][..]),
    };
    let attrs = parts.iter().filter_map(|part| match part {
        Part::Attr(_, value) => Some(value),
        _ => None,
    });
    match attrs.chain(values).find(|token| value::is_null(token)) {
        Some(token) => Err(token.error(
            "null only removes a prop on @set (@set <id> key=null); leave the key out here, or write \"null\" for the text",
        )),
        None => Ok(()),
    }
}

/// `@in parentId`: exactly one ID; the new nodes go on the indented lines below it.
fn within(d: DirectiveStmt) -> Result<Open> {
    if let Some(part) = d.parts.first() {
        let at = directives::token_of(part);
        return Err(at.error("@in takes only a parent ID; write its children on indented lines"));
    }
    let id = value::id(&d.positional[0])?;
    Ok(Open {
        anchor: id.clone(),
        id,
        children: 0,
        siblings: Siblings::default(),
        parent: None,
        within: Some(d.name),
        comp: false,
        // The stored parent's kind is not known here.
        page: false,
    })
}
