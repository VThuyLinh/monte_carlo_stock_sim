// file main.rs
use slint::{SharedString, Model, VecModel, SharedString as SlintString, ModelRc}; 
use anyhow::{Result, anyhow}; 
use std::time::Instant;
use std::rc::Rc; 
use std::sync::Mutex;
use std::fs::File;
use std::io::Write; 
use std::path::PathBuf; 
use plotters::prelude::*; // 👈 Dùng cho vẽ đồ thị
use plotters_slint::SlintDrawingArea; // 👈 Dùng cho vẽ đồ thị trên Slint

// Import logic crates
use data_io::{load_and_filter_data, estimate_parameters}; 
// THAY ĐỔI: Thêm get_available_tickers
use data_io::{get_available_tickers, StockRecord}; 
use core_sim::{simulate_gbm, simulate_bootstrap, calculate_metrics, SimulationResult}; 

slint::include_modules!(); 

// --- HÀM VẼ ĐỒ THỊ MỚI DÙNG PLOTTERS ---
/// Vẽ các đường giá mô phỏng lên Slint DrawingArea
fn plot_simulation_paths(
    // Slint DrawingArea: Lớp nền cho việc vẽ
    area: &SlintDrawingArea, 
    // Dữ liệu mô phỏng: Vec của các đường (path)
    simulation_data: &SimulationResult, 
    // Giá khởi điểm
    initial_price: f64
) -> Result<()> {
    // Kích thước của vùng vẽ
    let (width, height) = area.dim();
    // Tạo Chart Context
    let root = area.into_drawing_area();
    root.fill(&WHITE)?; // Nền trắng
    
    // Lấy min/max cho trục Y (Giá)
    let min_price = simulation_data.sample_paths.iter()
        .flat_map(|path| path.iter())
        .fold(initial_price, |min, &val| val.min(min));
    let max_price = simulation_data.sample_paths.iter()
        .flat_map(|path| path.iter())
        .fold(initial_price, |max, &val| val.max(max));
        
    // Lấy số ngày (trục X)
    let num_days = simulation_data.sample_paths.first()
        .map_or(0, |path| path.len() - 1); 

    let mut chart = ChartBuilder::on(&root)
        .caption("Monte Carlo Simulation Paths", ("sans-serif", 20).into_font())
        .margin(10)
        .x_label_area_size(30)
        .y_label_area_size(40)
        .build_cartesian_2d(
            0..num_days, 
            (min_price * 0.95)..(max_price * 1.05) // Thêm biên độ 5% cho trục Y
        )?;

    chart.configure_mesh().draw()?;

    // Vẽ từng đường giá (chỉ vẽ tối đa 50 đường cho đỡ nặng)
    let paths_to_plot = simulation_data.sample_paths.iter().take(50); 
    
    for (i, path) in paths_to_plot.enumerate() {
        let path_with_initial = std::iter::once(initial_price).chain(path.iter().cloned());
        
        let path_line = LineSeries::new(
            (0..=num_days).zip(path_with_initial),
            // Đặt màu xám nhẹ cho đường giá
            &HSLColor((i as f64) * 0.05, 0.7, 0.6)
        ).point_size(1);
        
        chart.draw_series(path_line)?.label(format!("Path {}", i + 1)).legend(|(x, y)| {
            PathElement::new(vec![(x, y), (x + 20, y)], &BLACK)
        });
    }

    root.present()?;
    Ok(())
}
// --- KẾT THÚC HÀM VẼ ĐỒ THỊ ---

fn export_metrics(summary: &str, file_name: &str) -> Result<()> {
    let mut file = File::create(file_name)?;
    file.write_all(summary.as_bytes())?;
    println!("✅ Xuất Summary thành công: {}", file_name);
    Ok(())
}

fn export_chart_placeholder(file_name: &str) -> Result<()> {
    // THAY ĐỔI: Giữ hàm này để tạo file rỗng nếu không có dữ liệu để vẽ
    println!("ℹ️ Đang xuất đồ thị ra file: {}", file_name);
    File::create(file_name)?; 
    Ok(())
}

fn main() -> Result<()> {
    let ui = AppWindow::new()?; 
    
    // --- KHAI BÁO BIẾN CHO PHẠM VI CHIA SẺ (Rc<Mutex<T>>) ---
    let historical_data = Rc::new(Mutex::new(Vec::<StockRecord>::new())); // Dùng StockRecord từ data_io
    let historical_returns = Rc::new(Mutex::new(Vec::<f64>::new()));
    let simulation_results = Rc::new(Mutex::new(Option::<SimulationResult>::None)); 
    // THÊM: Biến cho giá khởi điểm (cần cho vẽ đồ thị)
    let initial_price_ref = Rc::new(Mutex::new(0.0f64));

    // ----------------------------------------------------
    // TẢI DANH SÁCH TICKER VÀ GÁN VÀO UI
    // ----------------------------------------------------
    let file_path_str = ui.get_file_path().to_string(); // Lấy đường dẫn file mặc định
    match get_available_tickers(&file_path_str) {
        Ok(tickers) => {
            let ticker_models: Vec<SlintString> = tickers.iter()
                .map(|s| SlintString::from(s.as_str()))
                .collect();
            
            // Gán danh sách cho property mới: available_tickers
            ui.set_available_tickers(Rc::new(VecModel::from(ticker_models)).into()); 
            
            // Đặt Ticker mặc định là Ticker đầu tiên
            if let Some(default_ticker) = tickers.first() {
                ui.set_ticker(SlintString::from(default_ticker));
            }
        },
        Err(e) => {
            eprintln!("❌ Lỗi tải Ticker: {}", e);
            ui.set_data_info(SlintString::from(format!("❌ Lỗi tải Ticker: {}", e)));
        }
    }

    // ----------------------------------------------------
    // TẠO HANDLES RIÊNG (CLONE AS_WEAK) CHO TỪNG CALLBACK
    // ----------------------------------------------------
    
    // 1. Handles cho Load Data
    let ui_handle_load = ui.as_weak();
    let data_ref_load = historical_data.clone();
    let returns_ref_load = historical_returns.clone();
    let initial_price_ref_load = initial_price_ref.clone(); // Clone cho load

    // 2. Handles cho Run Simulation
    let ui_handle_run = ui.as_weak();
    let returns_ref_run = historical_returns.clone();
    let results_ref_run = simulation_results.clone();
    let initial_price_ref_run = initial_price_ref.clone(); // Clone cho run

    // 3. Handles cho Export Summary (giữ nguyên)
    let ui_handle_export_summary = ui.as_weak();
    // let summary_ref_export_summary = ui.get_metrics_summary(); // Không cần, lấy trực tiếp từ ui

    // 4. Handles cho Export Chart (giữ nguyên)
    let ui_handle_export_chart = ui.as_weak();
    let results_ref_export = simulation_results.clone(); 

    // 5. Handles cho Draw Chart (MỚI)
    let results_ref_draw = simulation_results.clone();
    let initial_price_ref_draw = initial_price_ref.clone();

    
    // --- 1. Load Data Handler (ĐÃ SỬA ĐỔI) ---
    ui.on_load_data(move || {
        let ui = ui_handle_load.unwrap();

        let file_path = ui.get_file_path();
        let ticker = ui.get_ticker(); // Lấy Ticker đang được chọn từ ComboBox

        // Bắt đầu quy trình Load & Filter như cũ
        match load_and_filter_data(&file_path.to_string(), &ticker.to_string()) {
            Ok(data) => {
                let end_date = data.last().map(|r| r.date.to_string()).unwrap_or_default();
                let start_date = data.first().map(|r| r.date.to_string()).unwrap_or_default();
                let last_close = data.last().map(|r| r.close).unwrap_or(0.0);
                let record_count = data.len();

                let mut data_lock = data_ref_load.lock().unwrap();
                *data_lock = data;
                let data_lock_clone = data_lock.clone();
                drop(data_lock);

                // LƯU Ý: Lưu giá khởi điểm vào biến chia sẻ
                *initial_price_ref_load.lock().unwrap() = last_close; 
                
                // ... (logic estimate_parameters và cập nhật GUI giữ nguyên) ...
                match estimate_parameters(&data_lock_clone) {
                    Ok(params) => {
                        let prices: Vec<f64> = data_lock_clone.iter().map(|r| r.close).collect();
                        let returns: Vec<f64> = prices.iter()
                            .zip(prices.iter().skip(1))
                            .map(|(prev, current)| (current / prev).ln())
                            .collect();
                        *returns_ref_load.lock().unwrap() = returns;
                        
                        // Cập nhật GUI
                        ui.set_data_info(SlintString::from(format!("✅ Ticker: {}, Date Range: {} - {}, Records: {}",
                            ticker, start_date, end_date, record_count)));
                        
                        let last_close_str = format!("{:.2}", last_close);
                        let mu_str = format!("{:.6}", params.mean_log_return_daily);
                        let sigma_str = format!("{:.6}", params.std_dev_log_return_daily);

                        ui.set_last_close_price_str(SlintString::from(last_close_str.clone()));
                        ui.set_daily_mu_est_str(SlintString::from(mu_str.clone()));
                        ui.set_daily_sigma_est_str(SlintString::from(sigma_str.clone()));
                        
                        // Cập nhật giá trị gợi ý cho Simulation
                        ui.set_initial_price_input_str(SlintString::from(last_close_str));
                        ui.set_mu_override_str(SlintString::from(mu_str));
                        ui.set_sigma_override_str(SlintString::from(sigma_str));

                        // Xóa kết quả mô phỏng cũ
                        ui.set_metrics_summary(SlintString::from("CHƯA CHẠY MÔ PHỎNG"));
                    },
                    Err(e) => {
                        ui.set_data_info(SlintString::from(format!("❌ Lỗi ước tính: {}", e)));
                    }
                }
            },
            Err(e) => {
                ui.set_data_info(SlintString::from(format!("❌ Lỗi tải CSV: {}", e)));
            }
        }
    });

    // --- 2. Simulation Runner Handler (ĐÃ SỬA ĐỔI) ---
    ui.on_run_simulation(move || {
        let ui = ui_handle_run.unwrap();
        let start_time = Instant::now();
        
        // ... (Parsing inputs giữ nguyên) ...
        let initial_price_str = ui.get_initial_price_input_str();
        let horizon_days_str = ui.get_horizon_days_str();
        let num_paths_str = ui.get_num_paths_str();
        let dt_str = ui.get_dt_input_str();
        let mu_override_str = ui.get_mu_override_str();
        let sigma_override_str = ui.get_sigma_override_str();
        let random_seed_str = ui.get_random_seed_str();
        let antithetic_variates = ui.get_antithetic_variates();
        let model_type = ui.get_model_type();

        let initial_price: f64 = match initial_price_str.parse() { Ok(v) => v, Err(_) => { ui.set_metrics_summary(SlintString::from("❌ Lỗi: Giá P0 không hợp lệ.")); return; } };
        let horizon_days: usize = match horizon_days_str.parse() { Ok(v) => v, Err(_) => { ui.set_metrics_summary(SlintString::from("❌ Lỗi: Horizon không hợp lệ.")); return; } };
        let num_paths: usize = match num_paths_str.parse() { Ok(v) => v, Err(_) => { ui.set_metrics_summary(SlintString::from("❌ Lỗi: Số paths không hợp lệ.")); return; } } ;
        let dt: f64 = match dt_str.parse() { Ok(v) => v, Err(_) => { ui.set_metrics_summary(SlintString::from("❌ Lỗi: dt không hợp lệ.")); return; } };
        let mu: f64 = match mu_override_str.parse() { Ok(v) => v, Err(_) => { ui.set_metrics_summary(SlintString::from("❌ Lỗi: Mu không hợp lệ.")); return; } };
        let sigma: f64 = match sigma_override_str.parse() { Ok(v) => v, Err(_) => { ui.set_metrics_summary(SlintString::from("❌ Lỗi: Sigma không hợp lệ.")); return; } };
        let seed: u64 = match random_seed_str.parse() { Ok(v) => v, Err(_) => { ui.set_metrics_summary(SlintString::from("❌ Lỗi: Seed không hợp lệ.")); return; } };

        // LƯU Ý: Cập nhật giá khởi điểm cho việc vẽ đồ thị
        *initial_price_ref_run.lock().unwrap() = initial_price; 

        let sim_result: Result<SimulationResult> = match model_type.as_str() {
            "GBM" => {
                simulate_gbm(initial_price, mu, sigma, horizon_days, num_paths, dt, seed, antithetic_variates)
            },
            "Bootstrap" => {
                let returns_lock = returns_ref_run.lock().unwrap();
                if returns_lock.is_empty() {
                    Err(anyhow!("Không có log-returns để chạy Bootstrap. Hãy Load & Estimate trước."))
                } else {
                    simulate_bootstrap(initial_price, &returns_lock, horizon_days, num_paths, seed)
                }
            },
            _ => Err(anyhow!("Mô hình không hợp lệ.")),
        };
        
        match sim_result {
            Ok(result) => {
                // CHÚ Ý: Xóa logic chuyển đổi dữ liệu đồ thị sang Slint ModelRc
                // Logic này KHÔNG CẦN THIẾT vì việc vẽ sẽ được xử lý bằng plotters/SlintDrawingArea
                // Tuy nhiên, nếu bạn vẫn muốn hiển thị data cho mục đích debug, bạn có thể giữ nó, 
                // nhưng `chart_paths` không được sử dụng cho ChartWidget nữa.
                
                // Vẫn lưu kết quả
                *results_ref_run.lock().unwrap() = Some(result.clone()); 

                let metrics = match calculate_metrics(&result.terminal_prices, initial_price) {
                    Ok(m) => m,
                    Err(e) => {
                        ui.set_metrics_summary(SlintString::from(format!("❌ Lỗi Metrics: {}", e)));
                        return;
                    }
                };
                
                // ... (Logic tính Execution Time và Summary giữ nguyên) ...
                let end_time = Instant::now();
                let elapsed_time = end_time - start_time;
                
                let summary = format!(
                    "✨ Metrics:\n\
                    Mean Return: {:.2}%\n\
                    Std Dev Return: {:.2}%\n\
                    Median Return: {:.2}%\n\
                    P5 (Worst 5%): {:.2}%\n\
                    P95 (Best 5%): {:.2}%\n\
                    VaR95 (%): {:.2}%\n\
                    VaR95 (Value): ${:.2}",
                    metrics.mean_return * 100.0,
                    metrics.std_dev_return * 100.0,
                    metrics.median_return * 100.0,
                    metrics.p5_return * 100.0,
                    metrics.p95_return * 100.0,
                    metrics.var95_percent,
                    metrics.var95_value
                );
                ui.set_metrics_summary(SlintString::from(summary));
                ui.set_execution_time(SlintString::from(format!("{:.2} ms", elapsed_time.as_micros() as f64 / 1000.0)));
                
                // Báo cho ChartWidget biết đã có kết quả mới để vẽ
                // (Chỉ cần gán giá trị rỗng vì data nằm trong simulation_results)
                ui.set_chart_paths(ModelRc::from(Rc::new(VecModel::from(vec![])))); 
            },
            Err(e) => {
                ui.set_metrics_summary(SlintString::from(format!("❌ Lỗi Mô phỏng: {}", e)));
            }
        }
    });

    // --- 3. Export Summary Handler (Giữ nguyên) ---
    // ...

    // --- 4. Export Chart Handler (Giữ nguyên) ---
    // ...

    // --- 5. Draw Chart Handler (MỚI) ---
    ui.global::<AppWindowHandles>().on_draw(move |area| {
        let results_lock = results_ref_draw.lock().unwrap();
        let initial_price_lock = initial_price_ref_draw.lock().unwrap();
        
        if let Some(result) = results_lock.as_ref() {
            // Gọi hàm vẽ đồ thị
            if let Err(e) = plot_simulation_paths(&area, result, *initial_price_lock) {
                eprintln!("❌ Lỗi vẽ đồ thị: {}", e);
            }
        } else {
             // Xóa nếu không có data
             if let Err(e) = area.into_drawing_area().fill(&WHITE) {
                 eprintln!("❌ Lỗi xóa vùng vẽ: {}", e);
             }
        }
    });
    
    // --- 6. Chạy UI ---
    ui.run()?;
    
    Ok(())
}