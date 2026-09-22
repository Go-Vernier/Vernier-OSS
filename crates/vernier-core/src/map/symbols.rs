//! Constants a service binds to literals, so a topic written as
//! `self.EXCHANGE` or `Queues.queueName` can be read back. Built from the
//! Setting facts of every file the service owns.
use std::collections::HashMap;

use super::facts::{Extraction, Fact};

/// service -> symbol name -> distinct literal values, in file order, at most
/// eight per name so a common name such as `url` cannot fan out wildly.
pub type Symbols = HashMap<String, HashMap<String, Vec<String>>>;

const MAX_VALUES: usize = 8;

pub fn build(extractions: &[(String, String, Extraction)]) -> Symbols {
    let mut symbols: Symbols = HashMap::new();
    for (service, _, ex) in extractions {
        let table = symbols.entry(service.clone()).or_default();
        for fact in &ex.facts {
            if let Fact::Setting { key, value, .. } = fact {
                let values = table.entry(key.clone()).or_default();
                if values.len() < MAX_VALUES && !values.contains(value) {
                    values.push(value.clone());
                }
            }
        }
    }
    symbols
}

/// The literals bound to an identifier path in this service. `expr` may be
/// `NAME`, `self.NAME`, `Queues.name`, or an annotation pair `key = Path.name`;
/// the last segment is the symbol. Anything with brackets, quotes or spaces
/// beyond that shape is not an identifier and resolves to nothing.
pub fn lookup<'a>(symbols: &'a Symbols, service: &str, expr: &str) -> Vec<&'a str> {
    let expr = expr.trim();
    let expr = expr.split_once('=').map_or(expr, |(_, rhs)| rhs.trim());
    let is_path = !expr.is_empty()
        && expr
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$'))
        && expr
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$');
    if !is_path {
        return Vec::new();
    }
    let name = expr
        .rsplit('.')
        .next()
        .unwrap_or(expr)
        .trim_start_matches('$');
    symbols
        .get(service)
        .and_then(|table| table.get(name))
        .map(|values| values.iter().map(String::as_str).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::facts::{Fact, Parser};
    use pretty_assertions::assert_eq;

    fn ex(service: &str, file: &str, facts: Vec<Fact>) -> (String, String, Extraction) {
        (
            service.into(),
            file.into(),
            Extraction {
                facts,
                parser: Parser::Regex,
                language: "x".into(),
            },
        )
    }
    fn setting(key: &str, value: &str) -> Fact {
        Fact::Setting {
            key: key.into(),
            value: value.into(),
            line: 1,
        }
    }

    #[test]
    fn symbols_are_per_service_and_resolve_member_paths() {
        let symbols = build(&[
            ex(
                "payment",
                "payment/rabbitmq.py",
                vec![
                    setting("EXCHANGE", "robot-shop"),
                    setting("ROUTING_KEY", "orders"),
                ],
            ),
            ex(
                "notifications",
                "notifications/Queues.java",
                vec![setting("queueName", "email")],
            ),
            ex(
                "notifications",
                "notifications/Other.java",
                vec![setting("queueName", "email"), setting("queueName", "sms")],
            ),
        ]);
        assert_eq!(
            lookup(&symbols, "payment", "self.EXCHANGE"),
            vec!["robot-shop"]
        );
        assert_eq!(lookup(&symbols, "payment", "ROUTING_KEY"), vec!["orders"]);
        assert_eq!(
            lookup(&symbols, "notifications", "queues = Queues.queueName"),
            vec!["email", "sms"]
        );
        assert_eq!(lookup(&symbols, "payment", "queueName"), Vec::<&str>::new());
        assert_eq!(
            lookup(&symbols, "payment", "foo(EXCHANGE)"),
            Vec::<&str>::new()
        );
        assert_eq!(
            lookup(&symbols, "payment", "['orders']"),
            Vec::<&str>::new()
        );
    }
}
