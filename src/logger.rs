use log::{Level, LevelFilter, Log, Metadata, Record};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::sync::Mutex;

tokio::task_local! {
    pub static CP_ID: usize;
}

pub struct FileLogger {
    writers: Mutex<HashMap<usize, Mutex<std::fs::File>>>,
    general_writer: Mutex<std::fs::File>,
}

impl FileLogger {
    pub fn new() -> Self {
        // Create logs directory if it doesn't exist
        if let Err(e) = fs::create_dir("./logs") {
            if e.kind() != std::io::ErrorKind::AlreadyExists {
                eprintln!("Failed to create logs directory: {}", e);
            }
        }

        // Always clear existing logs on startup
        if let Ok(entries) = fs::read_dir("./logs") {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if file_type.is_file() {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }

        // Create general log file
        let general_writer = OpenOptions::new()
            .create(true)
            .append(true)
            .open("./logs/general.log")
            .expect("Failed to open general log file");

        Self {
            writers: Mutex::new(HashMap::new()),
            general_writer: Mutex::new(general_writer),
        }
    }
}

impl Log for FileLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Info
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let message = format!(
            "{} [{}] {}: {}\n",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3f UTC"),
            record.level(),
            record.target(),
            record.args()
        );


        // Try to get CP_ID from task local
        if let Ok(cp_id) = CP_ID.try_with(|id| *id) {
                    
            
            let mut writers = self.writers.lock().unwrap();
            let writer = writers.entry(cp_id).or_insert_with(|| {
                let file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(format!("./logs/cp_{}.log", cp_id))
                    .expect("Failed to open CP log file");
                Mutex::new(file)
            });
            let mut file = writer.lock().unwrap();
            let _ = file.write_all(message.as_bytes());
            let _ = file.flush();
        } else {
            // Log to general file
          

            print!("{}", message); // Also print to console
            let mut file = self.general_writer.lock().unwrap();
            let _ = file.write_all(message.as_bytes());
            let _ = file.flush();
        }
    }

    fn flush(&self) {
        // Flush all writers
        let writers = self.writers.lock().unwrap();
        for writer in writers.values() {
            let _ = writer.lock().unwrap().flush();
        }
        let _ = self.general_writer.lock().unwrap().flush();
    }
}

pub fn init_logger() {
    let logger = FileLogger::new();
    log::set_boxed_logger(Box::new(logger)).expect("Failed to set logger");
    log::set_max_level(LevelFilter::Info);
}
