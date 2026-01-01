
use clap::Parser;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
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
    style::{Color, Style},
    widgets::{Block, Borders, Gauge, Paragraph},
    text::{Line, Span},
    Terminal,
};
use vector_engine::storage::mmap::MmapIndex;
use vector_engine::core::runtime::RuntimeConfig;
use rand::Rng;

#[derive(Parser, Debug)]
#[command(author, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    index: PathBuf,

    #[arg(short, long, default_value_t = 8)]
    concurrency: usize,
    
    #[arg(short, long, default_value_t = 60)]
    duration: u64,

    #[arg(short, long, default_value_t = 10)]
    k: usize,

    #[arg(short, long, default_value_t = 100)]
    ef: usize,
}

struct AppStats {
    total_queries: AtomicUsize,
    total_latency_us: AtomicUsize, // Approximate
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // 1. Load Index
    println!("Loading index from {:?}...", args.index);
    let index = Arc::new(MmapIndex::load(&args.index)?);
    println!("Index loaded. Warming up...");
    // Warmup already handled in load()

    let stats = Arc::new(AppStats {
        total_queries: AtomicUsize::new(0),
        total_latency_us: AtomicUsize::new(0),
    });

    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));

    // 2. Spawn Workers
    let mut handles = Vec::new();
    let dim = index.header().dimension as usize;
    
    // Get optimized core list (or fallback to 0..N)
    let core_order = RuntimeConfig::get_optimized_core_list()
        .unwrap_or_else(|| (0..args.concurrency).collect());
    
    println!("Using Core Affinity: {:?}", core_order.iter().take(args.concurrency).collect::<Vec<_>>());

    for i in 0..args.concurrency {
        let index_ref = index.clone();
        let stats_ref = stats.clone();
        let running_ref = running.clone(); // Running flag
        let k = args.k;
        let ef = args.ef;

        let core_id = if i < core_order.len() { core_order[i] } else { i };
        
        handles.push(thread::spawn(move || {
            // Pin Thread
            RuntimeConfig::pin_thread(core_id);
            
            let mut rng = rand::thread_rng();
            
            while running_ref.load(Ordering::Relaxed) {
                // Generate Random Query
                let query: Vec<f32> = (0..dim).map(|_| rng.gen::<f32>()).collect();
                
                let start = Instant::now();
                let _res = index_ref.search_two_stage(&query, k, ef);
                let elapsed = start.elapsed().as_micros() as usize;
                
                stats_ref.total_queries.fetch_add(1, Ordering::Relaxed);
                stats_ref.total_latency_us.fetch_add(elapsed, Ordering::Relaxed);
            }
        }));
    }

    // 3. TUI Loop
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app_start = Instant::now();
    let duration = Duration::from_secs(args.duration);

    loop {
        // Handle Input
        if event::poll(Duration::from_millis(100))? {
             if let Event::Key(key) = event::read()? {
                 if key.code == KeyCode::Char('q') {
                     break;
                 }
             }
        }

        let elapsed = app_start.elapsed();
        if elapsed >= duration {
            break;
        }

        let queries = stats.total_queries.load(Ordering::Relaxed);
        let latency_sum = stats.total_latency_us.load(Ordering::Relaxed);
        let qps = if elapsed.as_secs_f64() > 0.0 {
            queries as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };
        let avg_latency = if queries > 0 {
            latency_sum as f64 / queries as f64
        } else {
            0.0
        };

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints(
                    [
                        Constraint::Length(3),
                        Constraint::Length(3),
                        Constraint::Length(3),
                        Constraint::Percentage(50),
                    ]
                    .as_ref(),
                )
                .split(f.area());
                // But size() might still exist. Let's use f.size() for safety with older versions or check.
                // Ratatui 0.28+ suggests f.area().
                // I'll try f.area(). If fails, I fix.
                // Wait, crate says 0.30.
                
            let block = Block::default().title("Vector Engine Stress Test").borders(Borders::ALL);
            f.render_widget(block, chunks[0]);
            
            let time_left = duration.saturating_sub(elapsed);
            let time_text = format!("Time Remaining: {:.1}s", time_left.as_secs_f64());
            let p_time = Paragraph::new(time_text).style(Style::default().fg(Color::Cyan));
            f.render_widget(p_time, chunks[0].inner(ratatui::layout::Margin { vertical: 1, horizontal: 1 })); // Correct inner logic? 

            // QPS
            let qps_text = format!("QPS: {:.0}", qps);
            let p_qps = Paragraph::new(qps_text)
                .block(Block::default().title("Throughput").borders(Borders::ALL));
            f.render_widget(p_qps, chunks[1]);
            
            // Latency
            let lat_text = format!("Avg Latency: {:.2} µs", avg_latency);
            let p_lat = Paragraph::new(lat_text)
                .block(Block::default().title("Latency").borders(Borders::ALL));
            f.render_widget(p_lat, chunks[2]);

            // Progress
            let progress = elapsed.as_secs_f64() / duration.as_secs_f64();
            let gauge = Gauge::default()
                .block(Block::default().title("Progress").borders(Borders::ALL))
                .gauge_style(Style::default().fg(Color::Green))
                .ratio(progress.min(1.0));
            f.render_widget(gauge, chunks[3]);

        })?;
    }

    // Cleanup
    running.store(false, Ordering::Relaxed);
    for h in handles {
        h.join().unwrap();
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    println!("Test complete.");
    println!("Total Queries: {}", stats.total_queries.load(Ordering::Relaxed));
    
    Ok(())
}
