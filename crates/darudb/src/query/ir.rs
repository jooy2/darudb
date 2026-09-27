//! The query IR: the tree every query becomes, whether a builder or the query
//! language made it, and its encoding as a record for the language boundary
//! (`design/objects.md`, "The IR").

use crate::error::{Error, Result};
use crate::format::object::Value;
use crate::format::object::codec::{self, Raw};

/// How deeply expressions may nest. Every query is held to it, so that any
/// query fits in a record, whose nesting is bounded too, and so that running
/// one never recurses further.
pub(crate) const MAX_DEPTH: usize = 24;

/// An operator of an expression, with its code in the IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Between,
    In,
    Contains,
    StartsWith,
    EndsWith,
    IsNull,
}

impl Op {
    fn code(self) -> i64 {
        match self {
            Op::Eq => 4,
            Op::Ne => 5,
            Op::Lt => 6,
            Op::Le => 7,
            Op::Gt => 8,
            Op::Ge => 9,
            Op::Between => 10,
            Op::In => 11,
            Op::Contains => 12,
            Op::StartsWith => 13,
            Op::EndsWith => 14,
            Op::IsNull => 15,
        }
    }

    fn from_code(code: i64) -> Option<Self> {
        Some(match code {
            4 => Op::Eq,
            5 => Op::Ne,
            6 => Op::Lt,
            7 => Op::Le,
            8 => Op::Gt,
            9 => Op::Ge,
            10 => Op::Between,
            11 => Op::In,
            12 => Op::Contains,
            13 => Op::StartsWith,
            14 => Op::EndsWith,
            15 => Op::IsNull,
            _ => return None,
        })
    }

    /// How many values the operator takes, `None` for any number.
    fn arity(self) -> Option<usize> {
        match self {
            Op::IsNull => Some(0),
            Op::Between => Some(2),
            Op::In => None,
            _ => Some(1),
        }
    }

    /// The operator as the query language writes it.
    pub(crate) fn text(self) -> &'static str {
        match self {
            Op::Eq => "==",
            Op::Ne => "!=",
            Op::Lt => "<",
            Op::Le => "<=",
            Op::Gt => ">",
            Op::Ge => ">=",
            Op::Between => "BETWEEN",
            Op::In => "IN",
            Op::Contains => "CONTAINS",
            Op::StartsWith => "STARTSWITH",
            Op::EndsWith => "ENDSWITH",
            Op::IsNull => "IS NULL",
        }
    }
}

const AND: i64 = 1;
const OR: i64 = 2;
const NOT: i64 = 3;

/// An expression of a filter.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Expr {
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Not(Box<Expr>),
    /// A test of the value at `path`.
    Test {
        op: Op,
        path: Vec<String>,
        values: Vec<Value>,
    },
    /// A test with a parameter among its values, which a prepared query is
    /// given when it runs ([`Ir::bind`]). Planning refuses one.
    Prepared {
        op: Op,
        path: Vec<String>,
        values: Vec<Operand>,
    },
}

/// A value of a prepared test: a value, or the number of a parameter.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Operand {
    Value(Value),
    Param(usize),
}

impl Expr {
    /// `AND` over `terms`, with the terms that are themselves `AND`s taken
    /// apart, so that one query builds one tree however it is grouped.
    pub(crate) fn and(terms: impl IntoIterator<Item = Expr>) -> Expr {
        Expr::And(flatten(terms, |expr| match expr {
            Expr::And(terms) => Ok(terms),
            other => Err(other),
        }))
    }

    /// `OR` over `terms`, flattened like [`and`](Self::and).
    pub(crate) fn or(terms: impl IntoIterator<Item = Expr>) -> Expr {
        Expr::Or(flatten(terms, |expr| match expr {
            Expr::Or(terms) => Ok(terms),
            other => Err(other),
        }))
    }

    /// A test whose values may be parameters: a prepared test if any is,
    /// and a plain one otherwise.
    pub(crate) fn operands(op: Op, path: Vec<String>, values: Vec<Operand>) -> Expr {
        if values
            .iter()
            .any(|value| matches!(value, Operand::Param(_)))
        {
            return Expr::Prepared { op, path, values };
        }

        let values = values
            .into_iter()
            .map(|value| match value {
                Operand::Value(value) => value,
                Operand::Param(_) => unreachable!("a test without parameters"),
            })
            .collect();

        Expr::test(op, path, values)
    }

    /// The expression with `parameters` in place of its parameters.
    fn bind(&self, parameters: &[Value]) -> Result<Expr> {
        Ok(match self {
            Expr::And(terms) => Expr::and(
                terms
                    .iter()
                    .map(|term| term.bind(parameters))
                    .collect::<Result<Vec<_>>>()?,
            ),
            Expr::Or(terms) => Expr::or(
                terms
                    .iter()
                    .map(|term| term.bind(parameters))
                    .collect::<Result<Vec<_>>>()?,
            ),
            Expr::Not(term) => Expr::Not(Box::new(term.bind(parameters)?)),
            Expr::Test { .. } => self.clone(),
            Expr::Prepared { op, path, values } => Expr::test(
                *op,
                path.clone(),
                values
                    .iter()
                    .map(|value| match value {
                        Operand::Value(value) => Ok(value.clone()),
                        Operand::Param(index) => parameters
                            .get(*index)
                            .cloned()
                            .ok_or_else(|| missing(*index, parameters.len())),
                    })
                    .collect::<Result<_>>()?,
            ),
        })
    }

    /// Fails if a parameter of the expression is numbered `count` or more.
    fn check_parameters(&self, count: usize) -> Result<()> {
        match self {
            Expr::And(terms) | Expr::Or(terms) => terms
                .iter()
                .try_for_each(|term| term.check_parameters(count)),
            Expr::Not(term) => term.check_parameters(count),
            Expr::Test { .. } => Ok(()),
            Expr::Prepared { values, .. } => values.iter().try_for_each(|value| match value {
                Operand::Param(index) if *index >= count => Err(missing(*index, count)),
                _ => Ok(()),
            }),
        }
    }

    /// The number of the first parameter in the expression, if it has one.
    pub(crate) fn first_param(&self) -> Option<usize> {
        match self {
            Expr::And(terms) | Expr::Or(terms) => terms.iter().find_map(Expr::first_param),
            Expr::Not(term) => term.first_param(),
            Expr::Test { .. } => None,
            Expr::Prepared { values, .. } => values.iter().find_map(|value| match value {
                Operand::Param(index) => Some(*index),
                Operand::Value(_) => None,
            }),
        }
    }

    /// A test, with `== null` and `!= null` made the null tests they mean.
    pub(crate) fn test(op: Op, path: Vec<String>, values: Vec<Value>) -> Expr {
        match (op, values.as_slice()) {
            (Op::Eq, [Value::Null]) => Expr::Test {
                op: Op::IsNull,
                path,
                values: Vec::new(),
            },
            (Op::Ne, [Value::Null]) => Expr::Not(Box::new(Expr::Test {
                op: Op::IsNull,
                path,
                values: Vec::new(),
            })),
            _ => Expr::Test { op, path, values },
        }
    }
}

fn flatten(
    terms: impl IntoIterator<Item = Expr>,
    split: impl Fn(Expr) -> std::result::Result<Vec<Expr>, Expr>,
) -> Vec<Expr> {
    let mut out = Vec::new();

    for term in terms {
        match split(term) {
            Ok(inner) => out.extend(inner),
            Err(term) => out.push(term),
        }
    }

    out
}

/// A sort key: a path, and whether it sorts in descending order.
pub(crate) type SortKey = (Vec<String>, bool);

/// A query without its collection: what to find, in what order, how many.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Ir {
    pub(crate) filter: Option<Expr>,
    pub(crate) sort: Vec<SortKey>,
    pub(crate) offset: u64,
    pub(crate) limit: Option<u64>,
}

impl Ir {
    /// Whether the query has parameters.
    pub(crate) fn has_parameters(&self) -> bool {
        self.filter
            .as_ref()
            .is_some_and(|filter| filter.first_param().is_some())
    }

    /// Fails if a parameter is numbered `count` or more.
    pub(crate) fn check_parameters(&self, count: usize) -> Result<()> {
        self.filter
            .as_ref()
            .map_or(Ok(()), |filter| filter.check_parameters(count))
    }

    /// The query with `parameters` in place of its parameters; the query
    /// itself if it has none.
    pub(crate) fn bind(&self, parameters: &[Value]) -> Result<Ir> {
        Ok(Ir {
            filter: self
                .filter
                .as_ref()
                .map(|filter| filter.bind(parameters))
                .transpose()?,
            sort: self.sort.clone(),
            offset: self.offset,
            limit: self.limit,
        })
    }
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidQuery {
        message: message.into(),
    }
}

/// The error for parameter `index` of a query given `count`.
pub(crate) fn missing(index: usize, count: usize) -> Error {
    invalid(format!(
        "`${index}` names a parameter, and {count} were given"
    ))
}

/// The IR record of `ir` on collection `collection`, counting the objects
/// rather than returning them if `count` is set.
pub(crate) fn encode(collection: &str, ir: &Ir, count: bool) -> Result<Vec<u8>> {
    let mut fields = vec![(1, Raw::String(collection.to_owned()))];

    if let Some(filter) = &ir.filter {
        fields.push((2, encode_expr(filter)?));
    }

    if !ir.sort.is_empty() {
        fields.push((
            3,
            Raw::List(
                ir.sort
                    .iter()
                    .map(|(path, descending)| {
                        Raw::Object(vec![(1, encode_path(path)), (2, Raw::Bool(*descending))])
                    })
                    .collect(),
            ),
        ));
    }

    if ir.offset > 0 {
        fields.push((4, count_raw(ir.offset)?));
    }

    if let Some(limit) = ir.limit {
        fields.push((5, count_raw(limit)?));
    }

    if count {
        fields.push((6, Raw::Bool(true)));
    }

    Ok(codec::write(&fields))
}

fn count_raw(value: u64) -> Result<Raw> {
    i64::try_from(value)
        .map(Raw::Int)
        .map_err(|_| invalid(format!("{value} is too large for an offset or a limit")))
}

fn encode_path(path: &[String]) -> Raw {
    Raw::List(path.iter().cloned().map(Raw::String).collect())
}

fn encode_expr(expr: &Expr) -> Result<Raw> {
    let (code, subexpressions) = match expr {
        Expr::And(terms) => (AND, terms.as_slice()),
        Expr::Or(terms) => (OR, terms.as_slice()),
        Expr::Not(term) => (NOT, std::slice::from_ref(term.as_ref())),
        Expr::Test { op, path, values } => {
            let values: Vec<Operand> = values.iter().cloned().map(Operand::Value).collect();

            return encode_test(*op, path, &values);
        }
        Expr::Prepared { op, path, values } => return encode_test(*op, path, values),
    };
    let mut fields = vec![(1, Raw::Int(code))];

    if !subexpressions.is_empty() {
        fields.push((
            4,
            Raw::List(
                subexpressions
                    .iter()
                    .map(encode_expr)
                    .collect::<Result<_>>()?,
            ),
        ));
    }

    Ok(Raw::Object(fields))
}

fn encode_test(op: Op, path: &[String], values: &[Operand]) -> Result<Raw> {
    let mut fields = vec![(1, Raw::Int(op.code())), (2, encode_path(path))];

    if !values.is_empty() {
        fields.push((
            3,
            Raw::List(
                values
                    .iter()
                    .map(|value| match value {
                        Operand::Value(value) => value_raw(value),
                        // A parameter is an object holding its number, which
                        // no value a query compares with is.
                        Operand::Param(index) => Ok(Raw::Object(vec![(
                            1,
                            Raw::Int(
                                i64::try_from(*index)
                                    .map_err(|_| invalid("a parameter's number is too large"))?,
                            ),
                        )])),
                    })
                    .collect::<Result<_>>()?,
            ),
        ));
    }

    Ok(Raw::Object(fields))
}

/// A value a query compares with, as the IR holds it: a scalar.
fn value_raw(value: &Value) -> Result<Raw> {
    Ok(match value {
        Value::Bool(value) => Raw::Bool(*value),
        Value::Int(value) => Raw::Int(*value),
        Value::Float(value) => Raw::Float(*value),
        Value::String(value) => Raw::String(value.clone()),
        Value::Bytes(value) => Raw::Bytes(value.clone()),
        Value::Null => return Err(invalid("null is only compared with `==` or `!=`")),
        Value::List(_) | Value::Object(_) => {
            return Err(invalid(
                "a query compares with single values, not lists or objects",
            ));
        }
    })
}

/// Reads an IR record: the collection, the query, and whether it counts.
pub(crate) fn decode(bytes: &[u8]) -> Result<(String, Ir, bool)> {
    let fields = codec::read(bytes).map_err(|reason| invalid(format!("the IR: {reason}")))?;
    let mut collection = None;
    let mut ir = Ir::default();
    let mut count = false;

    for (id, raw) in fields {
        match (id, raw) {
            (1, Raw::String(name)) => collection = Some(name),
            (2, raw) => ir.filter = Some(decode_expr(raw)?),
            (3, Raw::List(keys)) => {
                for key in keys {
                    let Raw::Object(fields) = key else {
                        return Err(invalid("the IR holds a sort key that is not an object"));
                    };
                    let mut path = None;
                    let mut descending = false;

                    for (id, raw) in fields {
                        match (id, raw) {
                            (1, raw) => path = Some(decode_path(raw)?),
                            (2, Raw::Bool(value)) => descending = value,
                            _ => return Err(invalid("the IR holds a sort key it cannot read")),
                        }
                    }

                    ir.sort.push((
                        path.ok_or_else(|| invalid("the IR holds a sort key without a path"))?,
                        descending,
                    ));
                }
            }
            (4, Raw::Int(offset)) => ir.offset = count_of(offset)?,
            (5, Raw::Int(limit)) => ir.limit = Some(count_of(limit)?),
            (6, Raw::Bool(value)) => count = value,
            (id, _) => return Err(invalid(format!("the IR holds field {id}, which it cannot"))),
        }
    }

    let collection = collection.ok_or_else(|| invalid("the IR names no collection"))?;

    Ok((collection, ir, count))
}

fn count_of(value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| invalid("the IR holds a negative offset or limit"))
}

/// The path `raw` holds. Like the rest of the reading of the IR, it takes
/// the strings `codec::read` made rather than copying them: the records are
/// thrown away once read, and a binding's query is read on every call.
fn decode_path(raw: Raw) -> Result<Vec<String>> {
    let Raw::List(names) = raw else {
        return Err(invalid("the IR holds a path that is not a list"));
    };
    let path = names
        .into_iter()
        .map(|name| match name {
            Raw::String(name) if !name.is_empty() => Ok(name),
            _ => Err(invalid("the IR holds a path with a name that is not one")),
        })
        .collect::<Result<Vec<_>>>()?;

    if path.is_empty() {
        return Err(invalid("the IR holds an empty path"));
    }

    Ok(path)
}

/// Reads an expression. Its recursion is bounded by the nesting a record
/// allows, which `codec::read` has already held the IR to; the filter's own
/// limit is checked on the flattened tree when the query is planned.
fn decode_expr(raw: Raw) -> Result<Expr> {
    let Raw::Object(fields) = raw else {
        return Err(invalid("the IR holds an expression that is not an object"));
    };
    let mut code = None;
    let mut path = None;
    let mut values = Vec::new();
    let mut terms = Vec::new();

    for (id, raw) in fields {
        match (id, raw) {
            (1, Raw::Int(value)) => code = Some(value),
            (2, raw) => path = Some(decode_path(raw)?),
            (3, Raw::List(raw)) => {
                values = raw.into_iter().map(raw_operand).collect::<Result<_>>()?;
            }
            (4, Raw::List(raw)) => {
                terms = raw.into_iter().map(decode_expr).collect::<Result<_>>()?;
            }
            _ => return Err(invalid("the IR holds an expression it cannot read")),
        }
    }

    let code = code.ok_or_else(|| invalid("the IR holds an expression without an operator"))?;

    match code {
        AND | OR | NOT if path.is_some() || !values.is_empty() => Err(invalid(
            "the IR holds `AND`, `OR` or `NOT` with a path or values",
        )),
        AND => Ok(Expr::and(terms)),
        OR => Ok(Expr::or(terms)),
        NOT if terms.len() == 1 => Ok(Expr::Not(Box::new(terms.remove(0)))),
        NOT => Err(invalid(
            "the IR holds a `NOT` without exactly one expression",
        )),
        code => {
            let op = Op::from_code(code)
                .ok_or_else(|| invalid(format!("the IR holds the unknown operator {code}")))?;
            let path = path
                .ok_or_else(|| invalid(format!("the IR holds `{}` without a path", op.text())))?;

            if !terms.is_empty() || op.arity().is_some_and(|arity| arity != values.len()) {
                return Err(invalid(format!(
                    "the IR holds `{}` with the wrong operands",
                    op.text()
                )));
            }

            Ok(
                if values
                    .iter()
                    .any(|value| matches!(value, Operand::Param(_)))
                {
                    Expr::Prepared { op, path, values }
                } else {
                    Expr::Test {
                        op,
                        path,
                        values: values
                            .into_iter()
                            .filter_map(|value| match value {
                                Operand::Value(value) => Some(value),
                                Operand::Param(_) => None,
                            })
                            .collect(),
                    }
                },
            )
        }
    }
}

/// A value of a test as the IR holds it, or a parameter.
fn raw_operand(raw: Raw) -> Result<Operand> {
    match raw {
        Raw::Object(fields) => match fields.as_slice() {
            [(1, Raw::Int(index))] => usize::try_from(*index)
                .map(Operand::Param)
                .map_err(|_| invalid("the IR holds a parameter with a negative number")),
            _ => Err(invalid("the IR holds a value that is not a single value")),
        },
        raw => raw_value(raw).map(Operand::Value),
    }
}

/// The most parameters a record of parameters may count, so that a damaged
/// count cannot make reading one reserve more than it holds.
const MAX_PARAMETERS: usize = 1 << 16;

/// Reads the values of a prepared query's parameters as a binding sends
/// them: a record whose field 0 is how many there are, as an `int`, and field
/// `n + 1` the value of parameter `n`, a null one left out.
///
/// The fields go to their places as they are read: gathering them first made
/// and freed a vector on every run of a prepared query.
pub(crate) fn decode_parameters(bytes: &[u8]) -> Result<Vec<Value>> {
    let uncounted = || invalid("the parameters do not say how many there are");
    let mut values: Option<Vec<Value>> = None;

    codec::read_each(
        bytes,
        |id, raw| match &mut values {
            None => {
                let count = match (id, raw) {
                    (0, Raw::Int(count)) => usize::try_from(count)
                        .ok()
                        .filter(|count| *count <= MAX_PARAMETERS)
                        .ok_or_else(|| {
                            invalid(format!(
                                "a query takes from 0 to {MAX_PARAMETERS} parameters"
                            ))
                        })?,
                    _ => return Err(uncounted()),
                };

                values = Some(vec![Value::Null; count]);
                Ok(())
            }
            // Field ids ascend from 0, so every one after it is at least 1.
            Some(values) => {
                let slot = usize::try_from(id - 1)
                    .ok()
                    .and_then(|at| values.get_mut(at))
                    .ok_or_else(|| invalid("the parameters hold more values than they count"))?;

                *slot = match raw {
                    Raw::Bool(value) => Value::Bool(value),
                    Raw::Int(value) => Value::Int(value),
                    Raw::Float(value) => Value::Float(value),
                    Raw::String(value) => Value::String(value),
                    Raw::Bytes(value) => Value::Bytes(value),
                    _ => return Err(invalid("a parameter's value is not a single value")),
                };
                Ok(())
            }
        },
        |reason| invalid(format!("the parameters: {reason}")),
    )?;

    values.ok_or_else(uncounted)
}

fn raw_value(raw: Raw) -> Result<Value> {
    Ok(match raw {
        Raw::Bool(value) => Value::Bool(value),
        Raw::Int(value) => Value::Int(value),
        Raw::Float(value) => Value::Float(value),
        Raw::String(value) => Value::String(value),
        Raw::Bytes(value) => Value::Bytes(value),
        _ => return Err(invalid("the IR holds a value that is not a single value")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(text: &str) -> Vec<String> {
        text.split('.').map(str::to_owned).collect()
    }

    #[test]
    fn parameters_read_back_with_their_nulls() {
        let bytes = codec::write(&[
            (0, Raw::Int(4)),
            (1, Raw::String("a".into())),
            (3, Raw::Float(0.5)),
        ]);

        assert_eq!(
            decode_parameters(&bytes).unwrap(),
            [
                Value::from("a"),
                Value::Null,
                Value::Float(0.5),
                Value::Null
            ]
        );

        for refused in [
            codec::write(&[(1, Raw::Int(1))]),
            codec::write(&[(0, Raw::Int(-1))]),
            codec::write(&[(0, Raw::Int(1 << 20))]),
            codec::write(&[(0, Raw::Int(1)), (2, Raw::Int(1))]),
            codec::write(&[(0, Raw::Int(1)), (1, Raw::List(Vec::new()))]),
            codec::write(&[]),
            vec![0xFF],
        ] {
            let error = decode_parameters(&refused).unwrap_err();

            assert_eq!(error.code(), "INVALID_QUERY", "{refused:?}");
        }
    }

    #[test]
    fn a_query_reads_back_as_it_was_written() {
        let ir = Ir {
            filter: Some(Expr::or([
                Expr::and([
                    Expr::test(Op::Ge, path("age"), vec![Value::Int(18)]),
                    Expr::test(Op::StartsWith, path("name"), vec![Value::from("A")]),
                ]),
                Expr::Not(Box::new(Expr::test(
                    Op::In,
                    path("address.city"),
                    vec![Value::from("Seoul"), Value::from("Busan")],
                ))),
                Expr::test(Op::Eq, path("email"), vec![Value::Null]),
                Expr::test(Op::In, path("tags"), Vec::new()),
                Expr::test(
                    Op::Between,
                    path("score"),
                    vec![Value::Float(-0.5), Value::Float(f64::INFINITY)],
                ),
            ])),
            sort: vec![(path("age"), true), (path("name"), false)],
            offset: 5,
            limit: Some(10),
        };
        let bytes = encode("users", &ir, true).unwrap();

        assert_eq!(decode(&bytes).unwrap(), ("users".to_owned(), ir, true));
        assert_eq!(
            decode(&encode("users", &Ir::default(), false).unwrap()).unwrap(),
            ("users".to_owned(), Ir::default(), false)
        );
    }

    #[test]
    fn an_and_inside_an_and_decodes_flattened() {
        let a = Expr::test(Op::Eq, path("a"), vec![Value::Int(1)]);
        let b = Expr::test(Op::Eq, path("b"), vec![Value::Int(2)]);
        let c = Expr::test(Op::Eq, path("c"), vec![Value::Int(3)]);
        let nested = Ir {
            filter: Some(Expr::And(vec![
                Expr::And(vec![a.clone(), Expr::And(vec![b.clone()])]),
                c.clone(),
            ])),
            ..Ir::default()
        };
        let (_, decoded, _) = decode(&encode("c", &nested, false).unwrap()).unwrap();

        assert_eq!(decoded.filter, Some(Expr::And(vec![a, b, c])));
    }

    #[test]
    fn groupings_of_one_query_make_one_tree() {
        let a = Expr::test(Op::Eq, path("a"), vec![Value::Int(1)]);
        let b = Expr::test(Op::Eq, path("b"), vec![Value::Int(2)]);
        let c = Expr::test(Op::Eq, path("c"), vec![Value::Int(3)]);

        assert_eq!(
            Expr::and([Expr::and([a.clone(), b.clone()]), c.clone()]),
            Expr::and([a.clone(), Expr::and([b.clone(), c.clone()])])
        );
        assert_eq!(
            Expr::test(Op::Ne, path("a"), vec![Value::Null]),
            Expr::Not(Box::new(Expr::Test {
                op: Op::IsNull,
                path: path("a"),
                values: Vec::new()
            }))
        );
    }

    #[test]
    fn an_ir_that_cannot_be_a_query_is_invalid() {
        let test = |fields: Vec<(u64, Raw)>| {
            codec::write(&[(1, Raw::String("c".into())), (2, Raw::Object(fields))])
        };
        let path = (2, Raw::List(vec![Raw::String("a".into())]));
        let one = (3, Raw::List(vec![Raw::Int(1)]));
        let broken = [
            test(vec![(1, Raw::Int(99)), path.clone(), one.clone()]),
            test(vec![(1, Raw::Int(4)), path.clone()]),
            test(vec![(1, Raw::Int(4)), one.clone()]),
            test(vec![(1, Raw::Int(10)), path.clone(), one.clone()]),
            test(vec![(1, Raw::Int(3))]),
            test(vec![(1, Raw::Int(1)), path.clone()]),
            test(vec![
                (1, Raw::Int(4)),
                (2, Raw::List(Vec::new())),
                one.clone(),
            ]),
            test(vec![
                (1, Raw::Int(4)),
                path.clone(),
                (3, Raw::List(vec![Raw::List(Vec::new())])),
            ]),
            codec::write(&[(4, Raw::Int(-1)), (1, Raw::String("c".into()))]),
            codec::write(&[(2, Raw::Object(vec![(1, Raw::Int(1))]))]),
            vec![0xFF],
        ];

        for bytes in broken {
            assert_eq!(
                decode(&bytes).err().map(|error| error.code()),
                Some("INVALID_QUERY"),
                "{bytes:?}"
            );
        }

        // Nesting past what a record holds.
        let mut expr = Raw::Object(vec![(1, Raw::Int(15)), path.clone()]);

        for _ in 0..40 {
            expr = Raw::Object(vec![(1, Raw::Int(3)), (4, Raw::List(vec![expr]))]);
        }

        let deep = codec::write(&[(1, Raw::String("c".into())), (2, expr)]);

        assert_eq!(
            decode(&deep).err().map(|error| error.code()),
            Some("INVALID_QUERY")
        );
    }
}
