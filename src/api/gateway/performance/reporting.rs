use crate::etc::gate::resources::ProcessResources;
use opentelemetry_sdk::{
    error::OTelSdkResult,
    metrics::{
        PeriodicReader, SdkMeterProvider, Temporality,
        data::{AggregatedMetrics, MetricData, ResourceMetrics},
        exporter::PushMetricExporter,
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Default)]
struct Readings {
    counters: BTreeMap<String, u64>,
    histograms: BTreeMap<String, Value>,
    active: BTreeMap<String, i64>,
}

fn metric_key<'a>(
    name: &str,
    attributes: impl Iterator<Item = &'a opentelemetry::KeyValue>,
) -> String {
    let labels = attributes
        .map(|attribute| format!("{}={}", attribute.key, attribute.value))
        .collect::<Vec<_>>()
        .join(",");
    format!("{name}{{{labels}}}")
}

#[derive(Clone)]
struct Exporter(Arc<Mutex<Readings>>);
impl PushMetricExporter for Exporter {
    async fn export(&self, metrics: &ResourceMetrics) -> OTelSdkResult {
        let mut values = self.0.lock().unwrap();
        for metric in metrics.scope_metrics().flat_map(|scope| scope.metrics()) {
            match metric.data() {
                AggregatedMetrics::U64(MetricData::Sum(sum)) => {
                    for point in sum.data_points() {
                        values
                            .counters
                            .insert(metric_key(metric.name(), point.attributes()), point.value());
                    }
                }
                AggregatedMetrics::I64(MetricData::Sum(sum)) => {
                    for point in sum.data_points() {
                        values
                            .active
                            .insert(metric_key(metric.name(), point.attributes()), point.value());
                    }
                }
                AggregatedMetrics::F64(MetricData::Histogram(histogram)) => {
                    for point in histogram.data_points() {
                        values.histograms.insert(
                            metric_key(metric.name(), point.attributes()),
                            json!({"count":point.count(), "sum":point.sum()}),
                        );
                    }
                }
                AggregatedMetrics::U64(MetricData::Histogram(histogram)) => {
                    for point in histogram.data_points() {
                        values.histograms.insert(
                            metric_key(metric.name(), point.attributes()),
                            json!({"count":point.count(), "sum":point.sum()}),
                        );
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn force_flush(&self) -> OTelSdkResult {
        Ok(())
    }
    fn shutdown_with_timeout(&self, _: Duration) -> OTelSdkResult {
        Ok(())
    }
    fn temporality(&self) -> Temporality {
        Temporality::Cumulative
    }
}

pub(super) struct Metrics {
    provider: SdkMeterProvider,
    values: Arc<Mutex<Readings>>,
}
impl Metrics {
    pub(super) fn new() -> Self {
        let values = Arc::new(Mutex::new(Readings::default()));
        let reader = PeriodicReader::builder(Exporter(values.clone()))
            .with_interval(Duration::from_secs(3600))
            .build();
        let provider = SdkMeterProvider::builder().with_reader(reader).build();
        opentelemetry::global::set_meter_provider(provider.clone());
        Self { provider, values }
    }
    pub(super) fn counters(&self) -> BTreeMap<String, u64> {
        self.provider.force_flush().unwrap();
        self.values.lock().unwrap().counters.clone()
    }
    pub(super) fn histograms(&self) -> BTreeMap<String, Value> {
        self.provider.force_flush().unwrap();
        self.values.lock().unwrap().histograms.clone()
    }
    pub(super) fn active(&self) -> BTreeMap<String, i64> {
        self.provider.force_flush().unwrap();
        self.values.lock().unwrap().active.clone()
    }
}

pub(super) fn latencies(mut values: Vec<f64>) -> Value {
    if values.is_empty() {
        return Value::Null;
    }
    values.sort_by(f64::total_cmp);
    let at = |percent: usize| values[(values.len() * percent).div_ceil(100).saturating_sub(1)];
    json!({"p50":at(50), "p95":at(95), "p99":at(99), "max":values.last().unwrap()})
}

fn process_stats() -> (u64, f64) {
    let output = Command::new("ps")
        .args(["-o", "rss=,time=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    assert!(output.status.success());
    let output = String::from_utf8(output.stdout).unwrap();
    let mut columns = output.split_whitespace();
    let rss = columns.next().unwrap().parse().unwrap();
    let cpu = columns
        .next()
        .unwrap()
        .split(':')
        .fold(0.0, |seconds, part| {
            seconds * 60.0 + part.parse::<f64>().unwrap()
        });
    (rss, cpu)
}

pub(super) struct Sampler {
    stop: Arc<AtomicBool>,
    task: thread::JoinHandle<Value>,
}
impl Sampler {
    pub(super) fn start(resources: Arc<ProcessResources>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let cancelled = stop.clone();
        let metrics = tokio::runtime::Handle::current().metrics();
        let task = thread::spawn(move || {
            let start = Instant::now();
            let (_, initial_cpu) = process_stats();
            let mut rss_peak = 0;
            let mut task_peak = 0;
            let mut tracked_peak = 0;
            let mut primary_peak = 0;
            let mut mirror_peak = 0;
            let mut replay_peak = 0;
            let mut count = 0;
            while !cancelled.load(Ordering::Relaxed) {
                let available = resources.available();
                primary_peak =
                    primary_peak.max(resources.budgets.primary_concurrency - available.0);
                mirror_peak = mirror_peak.max(resources.budgets.mirror_concurrency - available.1);
                replay_peak = replay_peak.max(resources.budgets.replay_memory_bytes - available.2);
                task_peak = task_peak.max(metrics.num_alive_tasks());
                tracked_peak = tracked_peak.max(resources.tracked_tasks());
                if count % 10 == 0 {
                    rss_peak = rss_peak.max(process_stats().0);
                }
                count += 1;
                thread::sleep(Duration::from_millis(10));
            }
            let (rss, cpu) = process_stats();
            json!({"rss_peak_kib":rss_peak.max(rss), "cpu_percent_one_core":100.0 * (cpu - initial_cpu) / start.elapsed().as_secs_f64(), "tokio_tasks_peak":task_peak, "tracked_tasks_peak":tracked_peak, "primary_peak":primary_peak, "mirror_peak":mirror_peak, "replay_peak_bytes":replay_peak, "samples":count})
        });
        Self { stop, task }
    }
    pub(super) fn finish(self) -> Value {
        self.stop.store(true, Ordering::Relaxed);
        self.task.join().unwrap()
    }
}
