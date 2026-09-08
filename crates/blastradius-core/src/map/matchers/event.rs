//! Message producers and consumers, joined on what they name: a topic, a
//! queue, an exchange, a routing key, or the type of a typed event. The
//! resolver's topic index says who is on the other side. A client library
//! import also names the broker family, which gives the edge to the broker
//! itself when the repository declares one.
use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use super::{FileContext, Matcher};
use crate::map::config::ConfigIndex;
use crate::map::facts::{Arg, Extraction, Fact};
use crate::map::resolve::TopicSides;
use crate::map::symbols::{self, Symbols};
use crate::map::{Candidate, Target, TopicRole};
use crate::model::{EdgeType, Evidence};

pub struct Event;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub key: String,
    pub role: TopicRole,
    pub evidence: Evidence,
}

/// `send`/`sendAsync` alone is too common (`res.send`); it counts only when
/// the receiver names a messaging object.
const SEND_RECEIVERS: &[&str] = &[
    "producer",
    "kafka",
    "template",
    "publisher",
    "topic",
    "queue",
    "sqs",
    "sns",
    "pubsub",
    "eventhub",
    "bus",
    "stream",
    "bridge",
];
const PRODUCER_METHODS: &[&str] = &[
    "publish",
    "publishasync",
    "basic_publish",
    "basicpublish",
    "basicpublishasync",
    "produce",
    "produceasync",
    "sendtoqueue",
    "convertandsend",
    "sendmessage",
    "sendmessageasync",
    "sendmessagebatch",
    "putevents",
    "putrecord",
    "publishbatch",
];
const CONSUMER_METHODS: &[&str] = &[
    "subscribe",
    "subscribeasync",
    "psubscribe",
    "consume",
    "consumeasync",
    "basic_consume",
    "basicconsume",
    "basicconsumeasync",
    "receivemessage",
    "receivemessageasync",
    "addsubscription",
    "queuesubscribe",
    "subscribesync",
    "chansubscribe",
];
const DECLARE_METHODS: &[&str] = &[
    "queue_declare",
    "queuedeclare",
    "queuedeclareasync",
    "assertqueue",
    "queue_bind",
    "queuebind",
    "queuebindasync",
    "bindqueue",
    "exchange_declare",
    "exchangedeclare",
    "exchangedeclareasync",
    "assertexchange",
    "createtopic",
    "createqueue",
    "topicarn",
    "queueurl",
    "topic_arn",
    "queue_url",
    "topic",
    "subscription",
];
static PRODUCER_TYPES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(ProducerMessage|WriterConfig|kafka\.Writer|ProducerRecord|PublishRequest|SendMessageRequest|PublishCommand|SendMessageCommand)$").unwrap()
});
static CONSUMER_TYPES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(ConsumerMessage|ReaderConfig|kafka\.Reader|KafkaConsumer|SubscribeRequest|ReceiveMessageRequest|ReceiveMessageCommand)$").unwrap()
});
const CONSUMER_ANNOTATIONS: &[&str] = &[
    "KafkaListener",
    "RabbitListener",
    "JmsListener",
    "SqsListener",
    "StreamListener",
    "PulsarListener",
    "NatsListener",
    "ServiceBusTrigger",
    "QueueTrigger",
    "EventHubTrigger",
    "KafkaTrigger",
    "RabbitMQTrigger",
    "Incoming",
];
const PRODUCER_ANNOTATIONS: &[&str] = &["SendTo", "Outgoing"];
/// `new OrderPaidIntegrationEvent(...)`: constructing a typed event is
/// producing it. Domain events stay in-process.
static NEW_EVENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bnew\s+(?:[\w.]+\.)?([A-Z]\w*Event)\b").unwrap());
static ADD_SUBSCRIPTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"AddSubscription<\s*([A-Za-z_]\w*)").unwrap());
/// A handler's base type names what it consumes: `IIntegrationEventHandler<X>`,
/// `IConsumer<X>`. Single type argument only, so `IRequestHandler<A, B>` and
/// `MediatR`'s `INotificationHandler` (in-process) are left out.
static HANDLER_OF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^I?[A-Za-z_]\w*?(?:IntegrationEventHandler|EventHandler|MessageHandler|Consumer|HandleMessages)<\s*([A-Za-z_]\w*)\s*>$").unwrap()
});
static OBJECT_KEY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:topics?|queue(?:name|url)?|exchange|routing_?key|subject|channel|topic_?arn|destination)\b\s*[:=]\s*\[?\s*['"]([^'"]+)['"]"#).unwrap()
});
/// The list form of the same keys: `topics: ['alpha', 'beta']`. `OBJECT_KEY`
/// only ever captures the first quoted element of a `[...]` value, so this
/// regex isolates the whole list body and `QUOTED_IN_LIST` pulls every
/// element out of it.
static OBJECT_LIST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:topics|queues|subjects|channels)\s*[:=]\s*\[([^\]]*)\]").unwrap()
});
static QUOTED_IN_LIST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"['"]([^'"]+)['"]"#).unwrap());
static LIST_START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:\[|listOf\s*\(|List\.of\s*\(|Arrays\.asList\s*\(|\[\]string\s*\{|new\s*(?:string)?\s*\[\]|vec!\s*\[|Set\.of\s*\()").unwrap()
});
const STOP_KEYS: &[&str] = &[
    "true",
    "false",
    "null",
    "none",
    "nil",
    "utf-8",
    "utf8",
    "json",
    "application/json",
    "text/plain",
    "get",
    "post",
    "put",
    "delete",
    "patch",
    "error",
    "message",
    "data",
    "ok",
    "close",
    "connect",
    "end",
    "open",
    "ready",
    "exit",
    "string",
    "number",
    "object",
    "default",
    "id",
    "name",
    "type",
    "value",
    "key",
    "topic",
    "queue",
    "exchange",
    "test",
    "direct",
    "fanout",
    "headers",
    "localhost",
    // `new Event(...)` is the DOM's own event constructor, not a message
    // type; a real one always has a descriptive prefix (`OrderPaidEvent`).
    "event",
];

/// A plausible topic, queue, exchange, routing key or event type name.
pub fn is_topic_key(s: &str) -> bool {
    let n = s.len();
    (2..=120).contains(&n)
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | ':' | '*' | '#')
        })
        && s.chars().any(|c| c.is_ascii_alphabetic())
        && !s.starts_with(['/', '-', '.'])
        && !s.contains("://")
        && !STOP_KEYS.contains(&s.to_lowercase().as_str())
}

/// The broker a client library speaks to, from an import path.
pub fn broker_family(import_path: &str) -> Option<&'static str> {
    let p = import_path.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| p.contains(w));
    if has(&[
        "rabbitmq",
        "amqp",
        "pika",
        "bunny",
        "kombu",
        "masstransit.rabbit",
    ]) {
        Some("rabbitmq")
    } else if has(&["kafka", "sarama", "confluent"]) {
        Some("kafka")
    } else if has(&["nats"]) {
        Some("nats")
    } else if has(&["pulsar"]) {
        Some("pulsar")
    } else if has(&["mqtt", "paho"]) {
        Some("mqtt")
    } else if has(&["sqs"]) {
        Some("sqs")
    } else if has(&["sns"]) {
        Some("sns")
    } else if has(&["activemq", "artemis", "javax.jms", "jakarta.jms"]) {
        Some("activemq")
    } else if has(&["servicebus"]) {
        Some("servicebus")
    } else if has(&["eventhub"]) {
        Some("eventhub")
    } else {
        None
    }
}

/// `new ProducerRecord<>` -> (`new producerrecord`, `producerrecord`, empty):
/// the callee without generics, its last segment and its receiver, lowercased.
fn split_callee(callee: &str) -> (String, String, String) {
    let base = callee.split('<').next().unwrap_or(callee);
    let base = base.trim().trim_start_matches(['&', '*', '(']).trim();
    let base = base.strip_prefix("await ").unwrap_or(base).trim();
    let lower = base.to_lowercase();
    let last = lower
        .rsplit(['.', ':', '>'])
        .next()
        .unwrap_or(&lower)
        .to_string();
    let receiver = lower[..lower.len() - last.len()]
        .trim_end_matches(['.', ':', '>'])
        .to_string();
    (lower, last, receiver)
}

fn role_of_callee(callee: &str) -> Option<TopicRole> {
    let (base, last, receiver) = split_callee(callee);
    if PRODUCER_METHODS.contains(&last.as_str()) {
        return Some(TopicRole::Producer);
    }
    if (last == "send" || last == "sendasync")
        && SEND_RECEIVERS.iter().any(|r| receiver.contains(r))
    {
        return Some(TopicRole::Producer);
    }
    if CONSUMER_METHODS.contains(&last.as_str()) {
        return Some(TopicRole::Consumer);
    }
    if DECLARE_METHODS.contains(&last.as_str()) {
        return Some(TopicRole::Unknown);
    }
    if PRODUCER_TYPES.is_match(&base) {
        return Some(TopicRole::Producer);
    }
    if CONSUMER_TYPES.is_match(&base) {
        return Some(TopicRole::Consumer);
    }
    None
}

/// Topic keys named by a call's or annotation's arguments: string literals,
/// `new XEvent(...)`, object literals with a topic-ish key, lists of strings,
/// and identifiers looked up in the service's symbols.
fn keys_from_args(ctx: &FileContext<'_>, args: &[Arg]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |k: &str| {
        if is_topic_key(k) && !out.iter().any(|o| o == k) {
            out.push(k.to_string());
        }
    };
    for arg in args {
        match arg {
            Arg::Str(s) => push(s),
            Arg::Template(_) => {}
            Arg::Other(text) => {
                let text = text.trim();
                if let Some(caps) = NEW_EVENT.captures(text) {
                    if !caps[1].ends_with("DomainEvent") {
                        push(&caps[1]);
                    }
                } else if text.starts_with('{') {
                    for caps in OBJECT_KEY.captures_iter(text) {
                        push(&caps[1]);
                    }
                    for list in OBJECT_LIST.captures_iter(text) {
                        for caps in QUOTED_IN_LIST.captures_iter(&list[1]) {
                            push(&caps[1]);
                        }
                    }
                } else if LIST_START.is_match(text) {
                    for caps in QUOTED_IN_LIST.captures_iter(text) {
                        push(&caps[1]);
                    }
                } else {
                    for value in symbols::lookup(ctx.symbols, ctx.service, text) {
                        push(value);
                    }
                }
            }
        }
    }
    out
}

fn evidence(ctx: &FileContext<'_>, line: u32, detail: String) -> Evidence {
    Evidence {
        file: ctx.file.to_string(),
        line: Some(line),
        detail: Some(detail),
    }
}

/// Every topic mention in one file, in fact order.
pub fn mentions(ctx: &FileContext<'_>) -> Vec<Mention> {
    let mut out = Vec::new();
    let mut add = |key: String, role: TopicRole, line: u32, detail: String| {
        out.push(Mention {
            key,
            role,
            evidence: evidence(ctx, line, detail),
        });
    };
    for fact in ctx.facts {
        match fact {
            Fact::Call { callee, args, line } => {
                if let Some(caps) = ADD_SUBSCRIPTION.captures(callee) {
                    add(
                        caps[1].to_string(),
                        TopicRole::Consumer,
                        *line,
                        callee.clone(),
                    );
                    continue;
                }
                if let Some(rest) = callee.trim().strip_prefix("new ") {
                    let name = rest.split(['<', '(']).next().unwrap_or(rest).trim();
                    let name = name
                        .rsplit('.')
                        .next()
                        .unwrap_or(name)
                        .trim_end_matches("<>");
                    if name.ends_with("Event")
                        && !name.ends_with("DomainEvent")
                        && name.starts_with(|c: char| c.is_ascii_uppercase())
                        && is_topic_key(name)
                    {
                        add(name.to_string(), TopicRole::Producer, *line, callee.clone());
                        continue;
                    }
                }
                let Some(role) = role_of_callee(callee) else {
                    continue;
                };
                for key in keys_from_args(ctx, args) {
                    add(key, role, *line, callee.clone());
                }
            }
            Fact::Annotation { name, args, line } => {
                let short = name.rsplit('.').next().unwrap_or(name);
                let role = if CONSUMER_ANNOTATIONS.contains(&short) {
                    TopicRole::Consumer
                } else if PRODUCER_ANNOTATIONS.contains(&short) {
                    TopicRole::Producer
                } else {
                    continue;
                };
                for key in keys_from_args(ctx, args) {
                    add(key, role, *line, format!("@{short}"));
                }
            }
            Fact::Extends { name, line } => {
                if name.starts_with("INotificationHandler") || name.starts_with("IRequestHandler") {
                    continue;
                }
                if let Some(caps) = HANDLER_OF.captures(name.trim()) {
                    add(
                        caps[1].to_string(),
                        TopicRole::Consumer,
                        *line,
                        name.clone(),
                    );
                }
            }
            _ => {}
        }
    }
    out
}

/// key -> which services produce, consume or mention it.
pub fn topic_index(
    extractions: &[(String, String, Extraction)],
    config: &ConfigIndex,
    symbols: &Symbols,
) -> HashMap<String, TopicSides> {
    let mut index: HashMap<String, TopicSides> = HashMap::new();
    for (service, file, ex) in extractions {
        let ctx = FileContext {
            service,
            file,
            facts: &ex.facts,
            config,
            symbols,
        };
        for m in mentions(&ctx) {
            let sides = index.entry(m.key).or_default();
            let set = match m.role {
                TopicRole::Producer => &mut sides.producers,
                TopicRole::Consumer => &mut sides.consumers,
                TopicRole::Unknown => &mut sides.unknown,
            };
            set.insert(service.clone());
        }
    }
    index
}

impl Matcher for Event {
    fn name(&self) -> &'static str {
        "event"
    }

    fn candidates(&self, ctx: &FileContext<'_>) -> Vec<Candidate> {
        let mut out: Vec<Candidate> = mentions(ctx)
            .into_iter()
            .map(|m| Candidate {
                target: Target::Topic {
                    key: m.key,
                    role: m.role,
                },
                kind_hint: Some(EdgeType::Event),
                evidence: m.evidence,
            })
            .collect();
        let mut families: Vec<&str> = Vec::new();
        for fact in ctx.facts {
            if let Fact::Import { path, line } = fact {
                let Some(family) = broker_family(path) else {
                    continue;
                };
                if families.contains(&family) {
                    continue;
                }
                families.push(family);
                out.push(Candidate {
                    target: Target::Broker {
                        family: family.to_string(),
                        how: format!("imports {path} ({family} client)"),
                    },
                    kind_hint: Some(EdgeType::Event),
                    evidence: evidence(ctx, *line, format!("imports {path}")),
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::config::ConfigIndex;
    use crate::map::facts::Arg;
    use pretty_assertions::assert_eq;

    fn call(callee: &str, args: Vec<Arg>) -> Fact {
        Fact::Call {
            callee: callee.into(),
            args,
            line: 5,
        }
    }
    fn keys(ctx: &FileContext<'_>) -> Vec<(String, TopicRole)> {
        mentions(ctx).into_iter().map(|m| (m.key, m.role)).collect()
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn roles_and_keys_from_calls_annotations_and_base_types() {
        let mut symbols = Symbols::default();
        symbols
            .entry("payment".into())
            .or_default()
            .insert("EXCHANGE".into(), vec!["robot-shop".into()]);
        let facts = vec![
            call(
                "self._channel.basic_publish",
                vec![
                    Arg::Other("self.EXCHANGE".into()),
                    Arg::Str("orders".into()),
                    Arg::Other("body".into()),
                ],
            ),
            call(
                "ch.QueueDeclare",
                vec![Arg::Str("orders".into()), Arg::Other("true".into())],
            ),
            call("ch.Consume", vec![Arg::Str("orders".into())]),
            call(
                "producer.send",
                vec![Arg::Other(
                    "{ topic: 'order-created', messages: [{ value: 'hi' }] }".into(),
                )],
            ),
            call(
                "consumer.subscribe",
                vec![Arg::Other("{ topics: ['alpha', 'beta'] }".into())],
            ),
            call("consumer.subscribe", vec![Arg::Other("['orders']".into())]),
            call("res.send", vec![Arg::Str("ok".into())]),
            call(
                "kafkaTemplate.send",
                vec![Arg::Str("orders".into()), Arg::Other("m".into())],
            ),
            call("new ProducerRecord<>", vec![Arg::Str("orders".into())]),
            call("sarama.ProducerMessage", vec![Arg::Str("orders".into())]),
            call(
                "eventBus.AddSubscription<OrderPaidIntegrationEvent, OrderPaidIntegrationEventHandler>",
                vec![],
            ),
            call(
                "eventBus.PublishAsync",
                vec![Arg::Other("new OrderPaidIntegrationEvent(id)".into())],
            ),
            call(
                "new OrderStartedIntegrationEvent",
                vec![Arg::Other("userId".into())],
            ),
            call("new OrderStartedDomainEvent", vec![]),
            call(
                "observable.subscribe",
                vec![Arg::Other("x => go(x)".into())],
            ),
            Fact::Annotation {
                name: "KafkaListener".into(),
                args: vec![Arg::Str("order-created".into()), Arg::Str("g".into())],
                line: 9,
            },
            Fact::Annotation {
                name: "RabbitListener".into(),
                args: vec![Arg::Other("Queues.queueName".into())],
                line: 10,
            },
            Fact::Extends {
                name: "IIntegrationEventHandler<OrderPaidIntegrationEvent>".into(),
                line: 11,
            },
            Fact::Extends {
                name: "INotificationHandler<OrderStartedDomainEvent>".into(),
                line: 12,
            },
            Fact::Extends {
                name: "IRequestHandler<CreateOrderCommand, bool>".into(),
                line: 13,
            },
        ];
        let cfg = ConfigIndex::default();
        let ctx = FileContext {
            service: "payment",
            file: "payment/x.py",
            facts: &facts,
            config: &cfg,
            symbols: &symbols,
        };
        assert_eq!(
            keys(&ctx),
            vec![
                ("robot-shop".into(), TopicRole::Producer),
                ("orders".into(), TopicRole::Producer),
                ("orders".into(), TopicRole::Unknown),
                ("orders".into(), TopicRole::Consumer),
                ("order-created".into(), TopicRole::Producer),
                ("alpha".into(), TopicRole::Consumer),
                ("beta".into(), TopicRole::Consumer),
                ("orders".into(), TopicRole::Consumer),
                ("orders".into(), TopicRole::Producer),
                ("orders".into(), TopicRole::Producer),
                ("orders".into(), TopicRole::Producer),
                ("OrderPaidIntegrationEvent".into(), TopicRole::Consumer),
                ("OrderPaidIntegrationEvent".into(), TopicRole::Producer),
                ("OrderStartedIntegrationEvent".into(), TopicRole::Producer),
                ("order-created".into(), TopicRole::Consumer),
                ("OrderPaidIntegrationEvent".into(), TopicRole::Consumer),
            ]
        );
    }

    #[test]
    fn topic_keys_and_broker_families() {
        assert!(
            is_topic_key("orders")
                && is_topic_key("order.created")
                && is_topic_key("robot-shop")
                && is_topic_key("OrderPaidIntegrationEvent")
                && is_topic_key("arn:aws:sns:us-east-1:1:orders")
        );
        assert!(
            !is_topic_key("")
                && !is_topic_key("x")
                && !is_topic_key("ok")
                && !is_topic_key("/orders")
                && !is_topic_key("http://x")
                && !is_topic_key("hello world")
                && !is_topic_key("123")
                && !is_topic_key("utf-8")
                && !is_topic_key("${TOPIC}")
        );
        assert_eq!(broker_family("pika"), Some("rabbitmq"));
        assert_eq!(broker_family("github.com/streadway/amqp"), Some("rabbitmq"));
        assert_eq!(
            broker_family("org.springframework.amqp.rabbit.annotation.RabbitListener"),
            Some("rabbitmq")
        );
        assert_eq!(broker_family("kafkajs"), Some("kafka"));
        assert_eq!(broker_family("Confluent.Kafka"), Some("kafka"));
        assert_eq!(broker_family("github.com/IBM/sarama"), Some("kafka"));
        assert_eq!(broker_family("nats"), Some("nats"));
        assert_eq!(broker_family("@aws-sdk/client-sqs"), Some("sqs"));
        assert_eq!(broker_family("express"), None);
        assert_eq!(broker_family("redis"), None);
    }
}
