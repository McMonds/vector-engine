
use clap::Parser;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::thread;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style, Modifier},
    widgets::{Block, Borders, Gauge, Paragraph, Sparkline, Table, Row, Cell},
    text::{Line, Span},
    Terminal,
};
use vector_engine::storage::mmap::MmapIndex;
use vector_engine::core::runtime::RuntimeConfig;
use rand::Rng;
use sysinfo::{System, Pid};
use hdrhistogram::Histogram;

#[derive(Parser, Debug, Clone)]
#[command(author, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    index: PathBuf,

    #[arg(short, long)]
    concurrency: Option<usize>,
    
    #[arg(short, long, default_value_t = 60)]
    duration: u64,

    #[arg(short, long, default_value_t = 10)]
    k: usize,

    #[arg(short, long)]
    ef: Option<usize>,

    #[arg(long)]
    safe_mode: bool,
}

#[derive(PartialEq, Clone, Copy)]
enum AppState {
    Calibrating,
    Running,
    Analysis,
    Exiting,
}

struct AppStats {
    total_queries: AtomicUsize,
    total_latency_us: AtomicU64,
    min_latency_us: AtomicU64,
    max_latency_us: AtomicU64,
    latency_hist: Mutex<Histogram<u64>>,
    current_rss_kb: AtomicU64,
    peak_rss_kb: AtomicU64,
    
    // Throughput tracking
    peak_qps: Mutex<f64>,
    min_qps: Mutex<f64>,
}

struct HardwareInfo {
    cpu_brand: String,
    total_mem_mb: u64,
    logical_cores: usize,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // 1. Setup Data Structures
    let index = Arc::new(MmapIndex::load(&args.index)?);
    let stats = Arc::new(AppStats {
        total_queries: AtomicUsize::new(0),
        total_latency_us: AtomicU64::new(0),
        min_latency_us: AtomicU64::new(u64::MAX),
        max_latency_us: AtomicU64::new(0),
        latency_hist: Mutex::new(Histogram::<u64>::new(3).unwrap()),
        current_rss_kb: AtomicU64::new(0),
        peak_rss_kb: AtomicU64::new(0),
        peak_qps: Mutex::new(0.0),
        min_qps: Mutex::new(f64::MAX),
    });
    
    let mut sys_hw = System::new_all();
    sys_hw.refresh_all();
    let hw_info = HardwareInfo {
        cpu_brand: sys_hw.global_cpu_info().brand().trim().to_string(),
        total_mem_mb: sys_hw.total_memory() / (1024 * 1024),
        logical_cores: sys_hw.cpus().len(),
    };

    let state = Arc::new(Mutex::new(AppState::Calibrating));
    let running_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let calibrated_ef = Arc::new(AtomicUsize::new(args.ef.unwrap_or(64)));
    let is_auto_ef = args.ef.is_none();

    // 2. Resource Monitor
    let stats_mon = stats.clone();
    let flag_mon = running_flag.clone();
    thread::spawn(move || {
        let mut sys = System::new_all();
        let pid = Pid::from_u32(std::process::id());
        while flag_mon.load(Ordering::Relaxed) {
            sys.refresh_all();
            if let Some(process) = sys.process(pid) {
                let mem = process.memory();
                stats_mon.current_rss_kb.store(mem, Ordering::Relaxed);
                stats_mon.peak_rss_kb.fetch_max(mem, Ordering::Relaxed);
            }
            thread::sleep(Duration::from_millis(500));
        }
    });

    // 3. Auto-Tuning Engine (Phase 19: Saturate Strategy)
    let core_order = RuntimeConfig::get_optimized_core_list()
        .unwrap_or_else(|| (0..std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)).collect());
    
    let concurrency = args.concurrency.unwrap_or_else(|| {
        let total_cores = core_order.len();
        if args.safe_mode {
            if total_cores < 4 { 1 } else { total_cores / 2 }
        } else {
            total_cores // SATURATE BY DEFAULT
        }
    });

    // 4. Search Workers
    let dim = index.header().dimension as usize;
    let mut handles = Vec::new();
    for i in 0..concurrency {
        let index_ref = index.clone();
        let stats_ref = stats.clone();
        let flag_ref = running_flag.clone();
        let k = args.k;
        let ef_atomic = calibrated_ef.clone();
        let core_id = if i < core_order.len() { core_order[i] } else { i };

        handles.push(thread::spawn(move || {
            RuntimeConfig::pin_thread(core_id);
            let mut rng = rand::thread_rng();
            let mut local_hist = Histogram::<u64>::new(3).unwrap();
            let mut batch = 0;

            while !flag_ref.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(10));
            }

            while flag_ref.load(Ordering::Relaxed) {
                let ef = ef_atomic.load(Ordering::Relaxed);
                let query: Vec<f32> = (0..dim).map(|_| rng.gen::<f32>()).collect();
                let start = Instant::now();
                let _res = index_ref.search_two_stage(&query, k, ef);
                let lat = start.elapsed().as_micros() as u64;

                stats_ref.total_queries.fetch_add(1, Ordering::Relaxed);
                stats_ref.total_latency_us.fetch_add(lat, Ordering::Relaxed);
                stats_ref.min_latency_us.fetch_min(lat, Ordering::Relaxed);
                stats_ref.max_latency_us.fetch_max(lat, Ordering::Relaxed);
                local_hist.record(lat).ok();

                batch += 1;
                if batch >= 100 {
                    if let Ok(mut g) = stats_ref.latency_hist.try_lock() {
                        g.add(&local_hist).ok();
                        local_hist.reset();
                        batch = 0;
                    }
                }
            }
        }));
    }

    // 6. TUI Environment
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut start_time = Instant::now();
    let total_dur = Duration::from_secs(args.duration);
    let mut qps_hist = Vec::new();
    let mut steady_buffer: std::collections::VecDeque<f64> = std::collections::VecDeque::with_capacity(20);
    let min_burn_time = Duration::from_secs(5);
    let mut converged_time = None;
    let mut stability_score = 0.0;

    // Snapshot at finish
    let mut final_snapshot_queries = 0;
    let mut final_snapshot_elapsed = 0.0;
    let mut final_snapshot_qps = 0.0;

    loop {
        // App Controller
        let app_state = { *state.lock().unwrap() };
        if app_state == AppState::Exiting { break; }

        if app_state == AppState::Calibrating {
            if is_auto_ef {
                // Perform calibration in background or here? 
                // Let's do a simplified calibration to avoid blocking TUI for too long
                let truth_ef = 256;
                let mut best_ef = 64;
                
                // Sample queries
                let calibrate_queries: Vec<Vec<f32>> = (0..20).map(|_| {
                    let mut rng = rand::thread_rng();
                    (0..dim).map(|_| rng.gen::<f32>()).collect()
                }).collect();

                let ground_truth: Vec<Vec<usize>> = calibrate_queries.iter().map(|q| {
                    index.search_two_stage(q, args.k, truth_ef).into_iter().map(|(id, _)| id).collect()
                }).collect();

                    for test_ef in [16, 32, 48, 64, 80, 96, 128] {
                        let mut matches = 0;
                        let mut total = 0;
                        for (i, q) in calibrate_queries.iter().enumerate() {
                            let results: Vec<usize> = index.search_two_stage(q, args.k, test_ef).into_iter().map(|(id, _)| id).collect();
                            for id in &results {
                                if ground_truth[i].contains(id) { matches += 1; }
                            }
                            total += results.len();
                        }
                        best_ef = test_ef;
                        // Pareto Principle: 95% is the target for optimal speed/accuracy balance
                        if matches as f32 / total as f32 >= 0.95 { break; }
                    }
                    calibrated_ef.store(best_ef, Ordering::Release);
            }
            
            // Start the workers
            {
                let mut s = state.lock().unwrap();
                *s = AppState::Running;
                running_flag.store(true, Ordering::Release);
                start_time = Instant::now(); // Reset start time after calibration
            }
        }

        let is_running = {
            let mut s = state.lock().unwrap();
            if *s == AppState::Running && start_time.elapsed() >= total_dur {
                *s = AppState::Analysis;
                running_flag.store(false, Ordering::Relaxed);
                
                // FREEZE THE NUMBERS NOW
                final_snapshot_queries = stats.total_queries.load(Ordering::Relaxed);
                final_snapshot_elapsed = start_time.elapsed().as_secs_f64();
                final_snapshot_qps = final_snapshot_queries as f64 / final_snapshot_elapsed;
            }
            *s == AppState::Running
        };

        // Handle Input
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(k) = event::read()? {
                let mut s = state.lock().unwrap();
                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => *s = AppState::Exiting,
                    _ => {}
                }
            }
        }

        // Draw Frame
        let elapsed = if is_running { start_time.elapsed().as_secs_f64() } else { final_snapshot_elapsed };
        let queries = if is_running { stats.total_queries.load(Ordering::Relaxed) } else { final_snapshot_queries };
        let h = index.header();
        let cur_ef = calibrated_ef.load(Ordering::Relaxed);
        let visited_est = cur_ef + (h.num_elements as f64).sqrt() as usize;
        
        let qps = if is_running { 
            let cur_qps = if elapsed > 0.1 { queries as f64 / elapsed } else { 0.0 };
            
            // Update Peak/Min during run
            if cur_qps > 100.0 { // Skip ramp-up
                let mut peak = stats.peak_qps.lock().unwrap();
                if cur_qps > *peak { *peak = cur_qps; }
                let mut min = stats.min_qps.lock().unwrap();
                if cur_qps < *min { *min = cur_qps; }
            }
            cur_qps
        } else { 
            final_snapshot_qps 
        };
        
        let efficiency = qps / concurrency as f64;

        if qps_hist.len() > 100 { qps_hist.remove(0); }
        if is_running { 
            qps_hist.push(qps as u64);
            steady_buffer.push_back(qps);
            if steady_buffer.len() > 20 { steady_buffer.pop_front(); }
            
            // Steady State Detection (Phase 18)
            if steady_buffer.len() == 20 && start_time.elapsed() > min_burn_time {
                let mean: f64 = steady_buffer.iter().sum::<f64>() / 20.0;
                let variance: f64 = steady_buffer.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / 20.0;
                let std_dev = variance.sqrt();
                let cv = std_dev / mean; // Coefficient of Variation
                
                stability_score = (1.0 - cv).max(0.0) * 100.0;

                if cv < 0.02 {
                    // Converged!
                    let mut s = state.lock().unwrap();
                    if *s == AppState::Running {
                        converged_time = Some(start_time.elapsed());
                        *s = AppState::Analysis;
                        running_flag.store(false, Ordering::Relaxed);
                        final_snapshot_queries = stats.total_queries.load(Ordering::Relaxed);
                        final_snapshot_elapsed = start_time.elapsed().as_secs_f64();
                        final_snapshot_qps = final_snapshot_queries as f64 / final_snapshot_elapsed;
                    }
                }
            }
        }

        let rss = stats.current_rss_kb.load(Ordering::Relaxed) as f64 / (1024.0 * 1024.0);
        let peak_rss = stats.peak_rss_kb.load(Ordering::Relaxed) as f64 / (1024.0 * 1024.0);
        
        let mut p50 = 0; let mut p95 = 0; let mut p99 = 0;
        if let Ok(hist) = stats.latency_hist.lock() {
            p50 = hist.value_at_quantile(0.5);
            p95 = hist.value_at_quantile(0.95);
            p99 = hist.value_at_quantile(0.99);
        }

        // Visited Nodes estimate for throughput: ef + sqrt(N) heuristic
        let mb_s = (qps * (dim * visited_est) as f64) / 1_000_000.0;

        terminal.draw(|f| {
            let is_analysis = *state.lock().unwrap() == AppState::Analysis;
            
            let root = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(3), // Header
                    Constraint::Min(10),   // Metrics
                    Constraint::Length(3), // Progress
                ])
                .split(f.area());

            // Header
            let (status_color, status_text) = match app_state {
                AppState::Calibrating => (Color::Magenta, "🧠 ENGINE CALIBRATING..."),
                AppState::Running => (Color::Green, "⚡ ENGINE STRESS TEST RUNNING"),
                AppState::Analysis => (Color::Yellow, "📊 POST-TEST ANALYSIS MODE"),
                AppState::Exiting => (Color::Red, "EXITING..."),
            };
            let header = Paragraph::new(Line::from(vec![
                Span::styled(format!(" VECTOR ENGINE v2.1 | {:<30} | PRESS 'Q' TO EXIT ", status_text), Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
            ])).block(Block::default().borders(Borders::ALL));
            f.render_widget(header, root[0]);

            // Metrics Grid
            let body = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(root[1]);

            let bw_saturation = mb_s / 40000.0; // Assume 40GB/s as a baseline for "Max"
            let bw_color = if bw_saturation > 0.7 { Color::Red } 
                          else if bw_saturation < 0.1 { Color::Yellow }
                          else { Color::Cyan };

            // 21+ Metrics Table
            let metric_data = vec![
                ("Mean QPS".to_string(), format!("{:.0}", qps), Color::White),
                ("Peak QPS".to_string(), format!("{:.0}", *stats.peak_qps.lock().unwrap()), Color::Green),
                ("Min QPS".to_string(), format!("{:.0}", if *stats.min_qps.lock().unwrap() == f64::MAX { 0.0 } else { *stats.min_qps.lock().unwrap() }), Color::Yellow),
                ("Avg QPS/Thread".to_string(), format!("{:.0}", efficiency), Color::White),
                ("Total Queries".to_string(), format!("{}", queries), Color::White),
                ("Avg Latency".to_string(), format!("{:.1} µs", if queries > 0 { stats.total_latency_us.load(Ordering::Relaxed) as f64 / queries as f64 } else { 0.0 }), Color::White),
                ("Min Latency".to_string(), format!("{} µs", stats.min_latency_us.load(Ordering::Relaxed)), Color::White),
                ("Median (P50)".to_string(), format!("{} µs", p50), Color::White),
                ("P95 tail".to_string(), format!("{} µs", p95), Color::Yellow),
                ("P99 tail".to_string(), format!("{} µs", p99), Color::Red),
                ("Max Latency".to_string(), format!("{} µs", stats.max_latency_us.load(Ordering::Relaxed)), Color::Red),
                ("Current RSS".to_string(), format!("{:.2} MB", rss), Color::White),
                ("Peak RSS".to_string(), format!("{:.2} MB", peak_rss), Color::Magenta),
                ("Est. Bandwidth".to_string(), format!("{:.2} MB/s", mb_s), bw_color),
                ("---".to_string(), "---".to_string(), Color::DarkGray),
                ("Concurrency".to_string(), format!("{}", concurrency), Color::Cyan),
                ("Search EF".to_string(), format!("{}", calibrated_ef.load(Ordering::Relaxed)), Color::Cyan),
                ("---".to_string(), "---".to_string(), Color::DarkGray),
                ("Total Vectors (N)".to_string(), format!("{}", h.num_elements), Color::White),
                ("Dimensions".to_string(), format!("{}", h.dimension), Color::White),
                ("Max Graph Layer".to_string(), format!("{}", h.max_layer), Color::White),
                ("HNSW M (Neighbors)".to_string(), format!("{}", h.m_max), Color::White),
                ("Build EF".to_string(), format!("{}", h.ef_construction), Color::White),
                ("Search Concurrency".to_string(), format!("{}", concurrency), Color::White),
                ("Search EF".to_string(), format!("{}", calibrated_ef.load(Ordering::Relaxed)), Color::White),
                ("Search Top-K".to_string(), format!("{}", args.k), Color::White),
            ];

            let rows: Vec<Row> = metric_data.iter().map(|(m, v, col)| {
                Row::new(vec![
                    Cell::from(m.clone()),
                    Cell::from(v.clone()).style(Style::default().fg(*col)),
                ])
            }).collect();

            let table = Table::new(rows, [Constraint::Percentage(60), Constraint::Percentage(40)])
                .block(Block::default().title(" COMPREHENSIVE METRICS ").borders(Borders::ALL))
                .header(Row::new(vec!["Metric", "Value"]).style(Style::default().fg(Color::Cyan)));
            f.render_widget(table, body[0]);

            // Right side: Sparkline + Info
            let right_pane = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(8), Constraint::Min(2)])
                .split(body[1]);

            let spark = Sparkline::default()
                .block(Block::default().title(" THROUGHPUT HISTORY (100 pts) ").borders(Borders::ALL))
                .data(&qps_hist)
                .style(Style::default().fg(Color::Magenta));
            f.render_widget(spark, right_pane[0]);

            let info = Paragraph::new(format!(
                "DEVICE HARDWARE:\nCPU: {}\nCORES: {} Logical\nTOTAL RAM: {} MB\n\nTEST CONFIG:\nDURATION: {}s\nINDEX: {:?}\n\n[STATUS: {}]",
                hw_info.cpu_brand, hw_info.logical_cores, hw_info.total_mem_mb,
                args.duration, args.index, if is_analysis { "ANALYSIS" } else { "ACTIVE" }
            )).block(Block::default().title(" HARDWARE DIAGNOSTICS ").borders(Borders::ALL)).wrap(ratatui::widgets::Wrap { trim: true });
            f.render_widget(info, right_pane[1]);

            // Bottom Progress
            let is_analysis = app_state == AppState::Analysis;
            let ratio = if is_analysis { 1.0 } else { (elapsed / total_dur.as_secs_f64()).min(1.0) };
            let gauge_color = match app_state {
                AppState::Calibrating => Color::Magenta,
                AppState::Analysis => Color::DarkGray,
                _ => Color::Cyan,
            };
            let gauge = Gauge::default()
                .block(Block::default().title(" WORKLOAD COMPLETION "))
                .gauge_style(Style::default().fg(gauge_color))
                .ratio(ratio)
                .label(format!("{:.1}%", ratio * 100.0));
            f.render_widget(gauge, root[2]);
        })?;
    }

    // Done
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    // --- FINAL ANALYSIS LOGGING ---
    let final_peak_qps = *stats.peak_qps.lock().unwrap();
    let final_min_qps = if *stats.min_qps.lock().unwrap() == f64::MAX { 0.0 } else { *stats.min_qps.lock().unwrap() };
    let final_avg_lat = if final_snapshot_queries > 0 { stats.total_latency_us.load(Ordering::Relaxed) as f64 / final_snapshot_queries as f64 } else { 0.0 };
    let final_peak_mb = stats.peak_rss_kb.load(Ordering::Relaxed) as f64 / (1024.0 * 1024.0);
    
    let h = index.header(); // Ensure h is defined here for final calculations
    let final_ef = calibrated_ef.load(Ordering::Relaxed);
    let visited_est = final_ef + (h.num_elements as f64).sqrt() as usize;
    let final_bw = (final_snapshot_qps * (h.dimension as usize * visited_est) as f64) / 1_000_000.0;

    let mut f_p50 = 0; let mut f_p95 = 0; let mut f_p99 = 0;
    if let Ok(hist) = stats.latency_hist.lock() {
        f_p50 = hist.value_at_quantile(0.5);
        f_p95 = hist.value_at_quantile(0.95);
        f_p99 = hist.value_at_quantile(0.99);
    }

    println!("\n{}", "=".repeat(50));
    println!("        VECTOR ENGINE V2.1 - FINAL RESULTS");
    println!("{}", "=".repeat(50));
    println!("{:<25} : {}", "Total Vectors (N)", h.num_elements);
    println!("{:<25} : {}", "Dimensions", h.dimension);
    println!("{:<25} : {}", "Concurrency (Auto)", concurrency);
    println!("{:<25} : {}", "Search EF (Calibrated)", calibrated_ef.load(Ordering::Relaxed));
    println!("{}", "-".repeat(50));
    println!("{:<25} : {:.0} queries", "Total Queries", final_snapshot_queries);
    println!("{:<25} : {:.2} seconds", "Active Duration", final_snapshot_elapsed);
    println!("{}", "-".repeat(50));
    println!("{:<25} : {:.0} QPS", "Mean Throughput", final_snapshot_qps);
    println!("{:<25} : {:.0} QPS", "Peak Throughput", final_peak_qps);
    println!("{:<25} : {:.0} QPS", "Min Throughput", final_min_qps);
    println!("{:<25} : {:.2} MB/s", "Estimated Bandwidth", final_bw);
    println!("{}", "-".repeat(50));
    println!("{:<25} : {:.1} µs", "Avg Latency", final_avg_lat);
    println!("{:<25} : {} µs", "Min Latency", stats.min_latency_us.load(Ordering::Relaxed));
    println!("{:<25} : {} µs", "Median (P50)", f_p50);
    println!("{:<25} : {} µs", "P95 Tail", f_p95);
    println!("{:<25} : {} µs", "P99 Tail", f_p99);
    println!("{:<25} : {} µs", "Max Latency", stats.max_latency_us.load(Ordering::Relaxed));
    println!("{}", "-".repeat(50));
    println!("{:<25} : {:.2} MB", "Peak RSS Memory", final_peak_mb);
    if let Some(ct) = converged_time {
        println!("{:<25} : Workload Converged in {:.2}s", "Steady State", ct.as_secs_f64());
        println!("{:<25} : {:.2}%", "Stability Score", stability_score);
    } else {
        println!("{:<25} : Manual Exit / TimeoutReached", "Steady State");
    }
    println!("{:<25} : v2.1.0", "Engine Version");
    println!("{}", "=".repeat(50));
    Ok(())
}
