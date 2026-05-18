use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use auditaur_collector::exporter_sqlite::SqliteStore;
use auditaur_core::model::{LogRecord, SpanRecord, TelemetrySource};
use serde_json::{json, Value};
use tracing::{Event, Id, Subscriber};
use tracing_subscriber::{
    layer::{Context, Layer},
    registry::LookupSpan,
};
use uuid::Uuid;

static ACTIVE_SINK: Mutex<Option<TracingSink>> = Mutex::new(None);

#[derive(Clone)]
pub(crate) struct TracingSink {
    session_id: String,
    store: Arc<Mutex<SqliteStore>>,
}

pub(crate) fn install_sink(session_id: String, store: Arc<Mutex<SqliteStore>>) {
    let mut sink = ACTIVE_SINK.lock().expect("tracing sink lock poisoned");
    *sink = Some(TracingSink { session_id, store });
}

pub(crate) fn clear_sink(session_id: &str) {
    let Ok(mut sink) = ACTIVE_SINK.lock() else {
        return;
    };
    if sink
        .as_ref()
        .map(|sink| sink.session_id.as_str() == session_id)
        .unwrap_or(false)
    {
        *sink = None;
    }
}

pub fn tracing_layer() -> AuditaurTracingLayer {
    AuditaurTracingLayer { sink: None }
}

#[derive(Clone)]
pub struct AuditaurTracingLayer {
    sink: Option<TracingSink>,
}

impl std::fmt::Debug for AuditaurTracingLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditaurTracingLayer")
            .finish_non_exhaustive()
    }
}

impl AuditaurTracingLayer {
    pub(crate) fn with_sink(session_id: String, store: Arc<Mutex<SqliteStore>>) -> Self {
        Self {
            sink: Some(TracingSink { session_id, store }),
        }
    }

    fn sink(&self) -> Option<TracingSink> {
        self.sink.clone().or_else(active_sink)
    }
}

impl<S> Layer<S> for AuditaurTracingLayer
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(&self, attrs: &tracing::span::Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let parent = attrs
            .parent()
            .and_then(|parent| ctx.span(parent))
            .or_else(|| ctx.lookup_current());
        let parent_data =
            parent.and_then(|span| span.extensions().get::<AuditaurSpanData>().cloned());
        let attributes = span_fields_to_json(attrs);
        let trace_context = TraceContext::from_attributes(&attributes);
        let data = AuditaurSpanData {
            trace_id: trace_context
                .trace_id
                .or_else(|| parent_data.as_ref().map(|data| data.trace_id.clone()))
                .unwrap_or_else(random_trace_id),
            span_id: trace_context.span_id.unwrap_or_else(random_span_id),
            parent_span_id: trace_context
                .parent_span_id
                .or_else(|| parent_data.map(|data| data.span_id)),
            start_time_unix_nanos: now_unix_nanos(),
            attributes,
        };
        span.extensions_mut().insert(data);
    }

    fn on_record(&self, id: &Id, values: &tracing::span::Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut extensions = span.extensions_mut();
        let Some(data) = extensions.get_mut::<AuditaurSpanData>() else {
            return;
        };
        merge_json_objects(&mut data.attributes, span_record_to_json(values));
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(sink) = self.sink() else {
            return;
        };
        let Some(span) = ctx.span(&id) else {
            return;
        };
        let Some(data) = span.extensions().get::<AuditaurSpanData>().cloned() else {
            return;
        };
        let metadata = span.metadata();
        let record = SpanRecord {
            session_id: sink.session_id.clone(),
            trace_id: data.trace_id,
            span_id: data.span_id,
            parent_span_id: data.parent_span_id,
            name: metadata.name().to_string(),
            kind: data
                .attributes
                .get("otel.kind")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| Some("internal".to_string())),
            start_time_unix_nanos: data.start_time_unix_nanos,
            end_time_unix_nanos: Some(now_unix_nanos()),
            status_code: None,
            status_message: None,
            scope_name: Some(scope_name(metadata.target())),
            scope_version: None,
            attributes: data.attributes,
            source: TelemetrySource::Backend,
        };
        if let Ok(store) = sink.store.lock() {
            let _ = store.insert_span(&record);
        };
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let Some(sink) = self.sink() else {
            return;
        };
        let metadata = event.metadata();
        let fields = event_fields_to_json(event);
        let current_span = ctx.lookup_current();
        let span_data =
            current_span.and_then(|span| span.extensions().get::<AuditaurSpanData>().cloned());
        let record = LogRecord {
            session_id: sink.session_id.clone(),
            timestamp_unix_nanos: now_unix_nanos(),
            observed_timestamp_unix_nanos: None,
            severity_text: Some(metadata.level().to_string()),
            severity_number: Some(severity_number(metadata.level())),
            body: fields
                .get("message")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| Some(fields.to_string())),
            body_json: Some(fields.clone()),
            trace_id: span_data.as_ref().map(|data| data.trace_id.clone()),
            span_id: span_data.as_ref().map(|data| data.span_id.clone()),
            scope_name: Some(scope_name(metadata.target())),
            scope_version: None,
            attributes: json!({
                "auditaur.source": "backend",
                "code.namespace": metadata.target(),
                "code.filepath": metadata.file(),
                "code.lineno": metadata.line(),
            }),
            source: TelemetrySource::Backend,
        };
        if let Ok(store) = sink.store.lock() {
            let _ = store.insert_log(&record);
        };
    }
}

#[derive(Debug, Clone)]
struct AuditaurSpanData {
    trace_id: String,
    span_id: String,
    parent_span_id: Option<String>,
    start_time_unix_nanos: i64,
    attributes: Value,
}

#[derive(Default)]
struct TraceContext {
    trace_id: Option<String>,
    span_id: Option<String>,
    parent_span_id: Option<String>,
}

impl TraceContext {
    fn from_attributes(attributes: &Value) -> Self {
        let traceparent = attributes
            .get("traceparent")
            .and_then(Value::as_str)
            .and_then(parse_traceparent);
        Self {
            trace_id: attributes
                .get("otel.trace_id")
                .or_else(|| attributes.get("trace_id"))
                .and_then(Value::as_str)
                .filter(|value| is_hex_len(value, 32))
                .map(ToString::to_string)
                .or_else(|| traceparent.as_ref().map(|context| context.trace_id.clone())),
            span_id: attributes
                .get("otel.span_id")
                .or_else(|| attributes.get("span_id"))
                .and_then(Value::as_str)
                .filter(|value| is_hex_len(value, 16))
                .map(ToString::to_string),
            parent_span_id: attributes
                .get("otel.parent_span_id")
                .or_else(|| attributes.get("parent_span_id"))
                .and_then(Value::as_str)
                .filter(|value| is_hex_len(value, 16))
                .map(ToString::to_string)
                .or_else(|| traceparent.map(|context| context.parent_span_id)),
        }
    }
}

struct ParsedTraceparent {
    trace_id: String,
    parent_span_id: String,
}

fn parse_traceparent(value: &str) -> Option<ParsedTraceparent> {
    let mut parts = value.split('-');
    let version = parts.next()?;
    let trace_id = parts.next()?;
    let parent_span_id = parts.next()?;
    let flags = parts.next()?;
    if parts.next().is_some()
        || version.len() != 2
        || flags.len() != 2
        || !is_hex_len(trace_id, 32)
        || !is_hex_len(parent_span_id, 16)
    {
        return None;
    }
    Some(ParsedTraceparent {
        trace_id: trace_id.to_string(),
        parent_span_id: parent_span_id.to_string(),
    })
}

fn is_hex_len(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn active_sink() -> Option<TracingSink> {
    ACTIVE_SINK.lock().ok().and_then(|sink| sink.clone())
}

fn span_fields_to_json(fields: &tracing::span::Attributes<'_>) -> Value {
    let mut visitor = JsonVisitor::default();
    fields.record(&mut visitor);
    Value::Object(visitor.fields.into_iter().collect())
}

fn span_record_to_json(fields: &tracing::span::Record<'_>) -> Value {
    let mut visitor = JsonVisitor::default();
    fields.record(&mut visitor);
    Value::Object(visitor.fields.into_iter().collect())
}

fn event_fields_to_json(event: &Event<'_>) -> Value {
    let mut visitor = JsonVisitor::default();
    event.record(&mut visitor);
    Value::Object(visitor.fields.into_iter().collect())
}

fn merge_json_objects(target: &mut Value, source: Value) {
    let (Value::Object(target), Value::Object(source)) = (target, source) else {
        return;
    };
    for (key, value) in source {
        target.insert(key, value);
    }
}

#[derive(Default)]
struct JsonVisitor {
    fields: BTreeMap<String, Value>,
}

impl tracing::field::Visit for JsonVisitor {
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), Value::Bool(value));
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), Value::from(value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), Value::from(value));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.fields
            .insert(field.name().to_string(), Value::String(value.to_string()));
    }

    fn record_error(
        &mut self,
        field: &tracing::field::Field,
        value: &(dyn std::error::Error + 'static),
    ) {
        let mut sources = Vec::new();
        let mut source = value.source();
        while let Some(error) = source {
            sources.push(Value::String(error.to_string()));
            source = error.source();
        }
        self.fields.insert(
            field.name().to_string(),
            json!({
                "message": value.to_string(),
                "sources": sources,
            }),
        );
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.fields.insert(
            field.name().to_string(),
            Value::String(format!("{value:?}")),
        );
    }
}

fn severity_number(level: &tracing::Level) -> i64 {
    match *level {
        tracing::Level::TRACE => 1,
        tracing::Level::DEBUG => 5,
        tracing::Level::INFO => 9,
        tracing::Level::WARN => 13,
        tracing::Level::ERROR => 17,
    }
}

fn scope_name(target: &str) -> String {
    target.split("::").next().unwrap_or(target).to_string()
}

fn now_unix_nanos() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    i64::try_from(now.as_nanos()).unwrap_or(i64::MAX)
}

fn random_trace_id() -> String {
    Uuid::new_v4().simple().to_string()
}

fn random_span_id() -> String {
    Uuid::new_v4().simple().to_string()[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::tracing_layer;
    use crate::state::AuditaurState;
    use auditaur_collector::exporter_sqlite::SqliteStore;
    use auditaur_core::{
        storage::{LogQuery, SpanQuery},
        AuditaurConfig,
    };
    use tempfile::TempDir;
    use tracing_subscriber::{layer::SubscriberExt, Registry};

    #[test]
    fn tracing_events_and_spans_are_persisted() {
        let temp = TempDir::new().unwrap();
        let state = test_state(&temp);
        let session_id = state.session_id.clone().unwrap();
        let subscriber = Registry::default().with(state.tracing_layer());

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("work", item = 42);
            let _entered = span.enter();
            tracing::info!("hello from tracing");
        });

        let store = store_for(&temp, &session_id);
        let logs = store.list_logs(&LogQuery::default()).unwrap();
        let spans = store.list_spans(&SpanQuery::default()).unwrap();

        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].body.as_deref(), Some("hello from tracing"));
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].name, "work");
        assert_eq!(logs[0].trace_id, Some(spans[0].trace_id.clone()));
        assert_eq!(logs[0].span_id, Some(spans[0].span_id.clone()));
    }

    #[test]
    fn nested_spans_share_trace_and_record_parent() {
        let temp = TempDir::new().unwrap();
        let state = test_state(&temp);
        let session_id = state.session_id.clone().unwrap();
        let subscriber = Registry::default().with(state.tracing_layer());

        tracing::subscriber::with_default(subscriber, || {
            let root = tracing::info_span!("root");
            let _root = root.enter();
            {
                let child = tracing::info_span!("child");
                let _child = child.enter();
            }
        });

        let store = store_for(&temp, &session_id);
        let spans = store.list_spans(&SpanQuery::default()).unwrap();
        let root = spans.iter().find(|span| span.name == "root").unwrap();
        let child = spans.iter().find(|span| span.name == "child").unwrap();

        assert_eq!(child.trace_id, root.trace_id);
        assert_eq!(child.parent_span_id.as_deref(), Some(root.span_id.as_str()));
    }

    #[test]
    fn explicit_trace_context_fields_are_used() {
        let temp = TempDir::new().unwrap();
        let state = test_state(&temp);
        let session_id = state.session_id.clone().unwrap();
        let subscriber = Registry::default().with(state.tracing_layer());

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(
                "command",
                otel.trace_id = "00112233445566778899aabbccddeeff",
                otel.parent_span_id = "0011223344556677"
            );
            let _entered = span.enter();
        });

        let store = store_for(&temp, &session_id);
        let spans = store.list_spans(&SpanQuery::default()).unwrap();

        assert_eq!(spans[0].trace_id, "00112233445566778899aabbccddeeff");
        assert_eq!(spans[0].parent_span_id.as_deref(), Some("0011223344556677"));
    }

    #[test]
    fn global_layer_stops_writing_after_state_drop() {
        let temp = TempDir::new().unwrap();
        let state = test_state(&temp);
        let session_id = state.session_id.clone().unwrap();
        drop(state);
        let subscriber = Registry::default().with(tracing_layer());

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("after drop");
        });

        let store = store_for(&temp, &session_id);
        assert!(store.list_logs(&LogQuery::default()).unwrap().is_empty());
    }

    fn test_state(temp: &TempDir) -> AuditaurState {
        AuditaurState::initialize(
            AuditaurConfig {
                enabled: Some(true),
                service_name: Some("tracing-test".to_string()),
                data_dir: Some(temp.path().to_path_buf()),
                ..AuditaurConfig::default()
            },
            123,
            None,
        )
        .unwrap()
    }

    fn store_for(temp: &TempDir, session_id: &str) -> SqliteStore {
        let db = temp
            .path()
            .join("sessions")
            .join(session_id)
            .join("telemetry.sqlite");
        SqliteStore::open(db).unwrap()
    }
}
