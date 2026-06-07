use std::{error::Error, io, fs, process::{Command, Stdio}, sync::{Arc, Mutex}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}, path::PathBuf};
use crossterm::{event::{self, DisableMouseCapture, Event, KeyCode, KeyEventKind}, execute, terminal::*};
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use ratatui::{backend::CrosstermBackend, layout::*, style::*, widgets::*, Terminal, text::{Line, Span}};
use rodio::{Decoder, OutputStream, Sink, Source};

const APP_ID: &str = "1459887165784723673";
const EMBEDDED_SOUND: &[u8] = include_bytes!("../rain-sound.mp3");
const DEFAULT_ACTS: &[&str] = &["Studying 📚", "Coding 💻", "Deep Work 🧠", "Reading 📖"];

#[derive(Debug, PartialEq, Clone, Copy)]
enum Theme { Cyan, Magenta, Green, Yellow, Red }

impl Theme {
    fn color(&self) -> Color {
        match self {
            Theme::Cyan => Color::Cyan, Theme::Magenta => Color::Magenta,
            Theme::Green => Color::Green, Theme::Yellow => Color::Yellow, Theme::Red => Color::Red,
        }
    }
}

#[derive(PartialEq, Clone)]
enum Screen { Resume, Activity, Duration, Sessions, BGM, BGMImport, Settings, Timer, History, AddActivity, SessionForm, RenameActivity, Stats }

struct App {
    screen: Screen,
    acts: Vec<String>,
    idx: usize,
    mins: u32,
    break_mins: u32,
    total: u32,
    current: u32,
    rem: u32,
    work: bool,
    paused: bool,
    tick: Instant,
    input: String,
    status_msg: Arc<Mutex<String>>,
    is_downloading: Arc<Mutex<bool>>,
    download_done: Arc<Mutex<bool>>,
    bgm_list: Vec<String>,
    bgm_idx: usize,
    sink: Option<Sink>,
    _stream: Option<OutputStream>,
    notifications_enabled: bool,
    theme: Theme,
    settings_cursor: usize,
    volume: f32,
    muted: bool,
    data_dir: PathBuf,
    session_start: Option<Instant>,
    history_scroll: usize,
    // Session form fields
    form_act_idx: usize,
    form_mins: u32,
    form_break_mins: u32,
    form_done: u32,
    form_planned: u32,
    form_completed: bool,
    form_date: String,
    form_cursor: usize,
    form_edit_idx: Option<usize>,
    // Rename
    rename_idx: usize,
    // Stats
    stats_scroll: usize,
}

fn unix_to_date(secs: u64) -> String {
    let mut days = (secs / 86400) as i64;
    let mut year = 1970i64;
    loop {
        let days_in_year = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) { 366 } else { 365 };
        if days < days_in_year { break; }
        days -= days_in_year;
        year += 1;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut month = 0;
    for (i, &md) in month_days.iter().enumerate() {
        if days < md as i64 { month = i + 1; break; }
        days -= md as i64;
    }
    format!("{:04}-{:02}-{:02}", year, month, days + 1)
}

fn today_date() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    unix_to_date(now)
}

fn today_date_digits() -> String {
    today_date().replace("-", "")
}

fn format_date_display(digits: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = digits.chars().collect();
    for i in 0..8 {
        if i == 4 || i == 6 { result.push('-'); }
        if i < chars.len() { result.push(chars[i]); } else { result.push('_'); }
    }
    result
}

fn format_date_csv(digits: &str) -> String {
    if digits.len() == 8 {
        format!("{}-{}-{}", &digits[0..4], &digits[4..6], &digits[6..8])
    } else {
        format_date_display(digits)
    }
}

struct ActivityStats {
    name: String,
    total_entries: u32,
    total_sessions: u32,
    total_minutes: u32,
    completed: u32,
    abandoned: u32,
}

impl App {
    fn new() -> Self {
        let mut data_dir = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        data_dir.push("pomodoro-tui");
        let bgm_dir = data_dir.join("bgm");
        let _ = fs::create_dir_all(&bgm_dir);

        let rain_sound_path = bgm_dir.join("Rain_Background.mp3");
        if !rain_sound_path.exists() {
            let _ = fs::write(&rain_sound_path, EMBEDDED_SOUND);
        }

        let acts: Vec<String> = DEFAULT_ACTS.iter().map(|s| s.to_string()).collect();
        let has_saved_state = data_dir.join("timer_state.ini").exists();

        let mut app = Self {
            screen: if has_saved_state { Screen::Resume } else { Screen::Activity },
            acts,
            idx: 0, mins: 25, break_mins: 5, total: 4, current: 1, rem: 25 * 60,
            work: true, paused: true, tick: Instant::now(),
            input: String::new(),
            status_msg: Arc::new(Mutex::new("Ready".into())),
            is_downloading: Arc::new(Mutex::new(false)),
            download_done: Arc::new(Mutex::new(false)),
            bgm_list: vec!["None".into()], bgm_idx: 0, sink: None, _stream: None,
            notifications_enabled: true,
            theme: Theme::Cyan,
            settings_cursor: 0,
            volume: 0.5,
            muted: false,
            data_dir,
            session_start: None,
            history_scroll: 0,
            form_act_idx: 0, form_mins: 25, form_break_mins: 5,
            form_done: 4, form_planned: 4, form_completed: true,
            form_date: today_date_digits(), form_cursor: 0, form_edit_idx: None,
            rename_idx: 0,
            stats_scroll: 0,
        };
        app.load_config();
        app.refresh_bgm();
        app
    }

    fn config_path(&self) -> PathBuf { self.data_dir.join("config.ini") }
    fn state_path(&self) -> PathBuf { self.data_dir.join("timer_state.ini") }

    fn load_config(&mut self) {
        if let Ok(content) = fs::read_to_string(self.config_path()) {
            for line in content.lines() {
                let parts: Vec<&str> = line.splitn(2, '=').collect();
                if parts.len() != 2 { continue; }
                let (key, val) = (parts[0].trim(), parts[1].trim());
                match key {
                    "break_mins" => { if let Ok(v) = val.parse::<u32>() { self.break_mins = v.max(1); } }
                    "notifications" => { self.notifications_enabled = val == "true"; }
                    "theme" => { self.theme = match val { "Magenta" => Theme::Magenta, "Green" => Theme::Green, "Yellow" => Theme::Yellow, "Red" => Theme::Red, _ => Theme::Cyan }; }
                    "volume" => { if let Ok(v) = val.parse::<u32>() { self.volume = (v as f32 / 100.0).clamp(0.0, 1.0); } }
                    "activity" => {
                        let trimmed = val.to_string();
                        if !trimmed.is_empty() && !self.acts.contains(&trimmed) {
                            self.acts.push(trimmed);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn save_config(&self) {
        use std::io::Write;
        if let Ok(mut file) = fs::File::create(self.config_path()) {
            let _ = writeln!(file, "break_mins={}", self.break_mins);
            let _ = writeln!(file, "notifications={}", self.notifications_enabled);
            let _ = writeln!(file, "theme={:?}", self.theme);
            let _ = writeln!(file, "volume={}", (self.volume * 100.0) as u32);
            for act in self.acts.iter().skip(DEFAULT_ACTS.len()) {
                let _ = writeln!(file, "activity={}", act);
            }
        }
    }

    fn save_timer_state(&self) {
        use std::io::Write;
        if let Ok(mut file) = fs::File::create(self.state_path()) {
            let _ = writeln!(file, "idx={}", self.idx);
            let _ = writeln!(file, "mins={}", self.mins);
            let _ = writeln!(file, "break_mins={}", self.break_mins);
            let _ = writeln!(file, "total={}", self.total);
            let _ = writeln!(file, "current={}", self.current);
            let _ = writeln!(file, "rem={}", self.rem);
            let _ = writeln!(file, "work={}", self.work);
            let _ = writeln!(file, "bgm_idx={}", self.bgm_idx);
        }
    }

    fn load_timer_state(&mut self) -> bool {
        if let Ok(content) = fs::read_to_string(self.state_path()) {
            for line in content.lines() {
                let parts: Vec<&str> = line.splitn(2, '=').collect();
                if parts.len() != 2 { continue; }
                let (key, val) = (parts[0].trim(), parts[1].trim());
                match key {
                    "idx" => { if let Ok(v) = val.parse::<usize>() { if v < self.acts.len() { self.idx = v; } } }
                    "mins" => { if let Ok(v) = val.parse::<u32>() { self.mins = v.max(1); } }
                    "break_mins" => { if let Ok(v) = val.parse::<u32>() { self.break_mins = v.max(1); } }
                    "total" => { if let Ok(v) = val.parse::<u32>() { self.total = v.max(1); } }
                    "current" => { if let Ok(v) = val.parse::<u32>() { self.current = v.max(1); } }
                    "rem" => { if let Ok(v) = val.parse::<u32>() { self.rem = v; } }
                    "work" => { self.work = val == "true"; }
                    "bgm_idx" => { if let Ok(v) = val.parse::<usize>() { self.bgm_idx = v; } }
                    _ => {}
                }
            }
            true
        } else { false }
    }

    fn clear_timer_state(&self) { let _ = fs::remove_file(self.state_path()); }

    fn sort_sessions_csv(&self) {
        let log_path = self.data_dir.join("sessions.csv");
        if let Ok(content) = fs::read_to_string(&log_path) {
            let lines: Vec<&str> = content.lines().collect();
            if lines.len() <= 2 { return; } // header + 0-1 entries, no need to sort
            let header = lines[0];
            let mut data: Vec<&str> = lines[1..].iter().copied().filter(|l| !l.is_empty()).collect();
            data.sort_by(|a, b| {
                let date_a = a.split(',').next().unwrap_or("");
                let date_b = b.split(',').next().unwrap_or("");
                date_a.cmp(date_b)
            });
            let mut result = vec![header.to_string()];
            for line in data { result.push(line.to_string()); }
            let _ = fs::write(&log_path, result.join("\n") + "\n");
        }
    }

    // Menu: activities + "Previous Sessions" + "Session Stats"
    fn menu_len(&self) -> usize { self.acts.len() + 2 }

    fn refresh_bgm(&mut self) {
        let mut list = vec!["None".into()];
        let bgm_dir = self.data_dir.join("bgm");
        if let Ok(entries) = fs::read_dir(bgm_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.ends_with(".mp3") { list.push(name); }
            }
        }
        self.bgm_list = list;
    }

    fn log_session(&self, completed: bool) {
        let log_path = self.data_dir.join("sessions.csv");
        let write_header = !log_path.exists();
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&log_path) {
            use std::io::Write;
            if write_header {
                let _ = writeln!(file, "date,activity,duration_mins,break_mins,sessions_planned,sessions_done,completed");
            }
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
            let date = unix_to_date(now);
            let activity = self.acts[self.idx].replace(",", "");
            let sessions_done = if completed { self.total } else { self.current.saturating_sub(1) };
            let _ = writeln!(file, "{},{},{},{},{},{},{}", date, activity, self.mins, self.break_mins, self.total, sessions_done, if completed { "Yes" } else { "No" });
        }
        self.sort_sessions_csv();
    }

    fn log_manual_session(&self) {
        let log_path = self.data_dir.join("sessions.csv");
        let write_header = !log_path.exists();
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&log_path) {
            use std::io::Write;
            if write_header {
                let _ = writeln!(file, "date,activity,duration_mins,break_mins,sessions_planned,sessions_done,completed");
            }
            let activity = if self.form_act_idx < self.acts.len() {
                self.acts[self.form_act_idx].replace(",", "")
            } else { "Unknown".to_string() };
            let _ = writeln!(file, "{},{},{},{},{},{},{}", format_date_csv(&self.form_date), activity, self.form_mins, self.form_break_mins, self.form_planned, self.form_done, if self.form_completed { "Yes" } else { "No" });
        }
        self.sort_sessions_csv();
    }

    fn update_session_entry(&self, display_idx: usize) {
        let log_path = self.data_dir.join("sessions.csv");
        if let Ok(content) = fs::read_to_string(&log_path) {
            let all_lines: Vec<&str> = content.lines().collect();
            let data_count = all_lines.len().saturating_sub(1);
            if data_count == 0 || display_idx >= data_count { return; }
            let file_line_idx = data_count - display_idx;
            let activity = if self.form_act_idx < self.acts.len() {
                self.acts[self.form_act_idx].replace(",", "")
            } else { "Unknown".to_string() };
            let new_line = format!("{},{},{},{},{},{},{}", format_date_csv(&self.form_date), activity, self.form_mins, self.form_break_mins, self.form_planned, self.form_done, if self.form_completed { "Yes" } else { "No" });
            let mut new_lines: Vec<String> = all_lines.iter().map(|s| s.to_string()).collect();
            if file_line_idx < new_lines.len() {
                new_lines[file_line_idx] = new_line;
            }
            let _ = fs::write(&log_path, new_lines.join("\n") + "\n");
            self.sort_sessions_csv();
        }
    }

    fn rename_activity_in_csv(&self, old_name: &str, new_name: &str) {
        let log_path = self.data_dir.join("sessions.csv");
        if let Ok(content) = fs::read_to_string(&log_path) {
            let mut new_lines: Vec<String> = Vec::new();
            for (i, line) in content.lines().enumerate() {
                if i == 0 { new_lines.push(line.to_string()); continue; }
                let parts: Vec<&str> = line.split(',').collect();
                if parts.len() >= 7 && parts[1] == old_name {
                    let mut updated = parts.iter().map(|s| s.to_string()).collect::<Vec<_>>();
                    updated[1] = new_name.to_string();
                    new_lines.push(updated.join(","));
                } else {
                    new_lines.push(line.to_string());
                }
            }
            let _ = fs::write(&log_path, new_lines.join("\n") + "\n");
        }
    }

    fn load_stats(&self) -> Vec<ActivityStats> {
        let log_path = self.data_dir.join("sessions.csv");
        let mut stats_map: Vec<(String, u32, u32, u32, u32, u32)> = Vec::new(); // name, entries, sessions, minutes, completed, abandoned

        if let Ok(content) = fs::read_to_string(&log_path) {
            for (i, line) in content.lines().enumerate() {
                if i == 0 { continue; }
                let parts: Vec<&str> = line.split(',').collect();
                if parts.len() >= 7 {
                    let name = parts[1].to_string();
                    let mins: u32 = parts[2].parse().unwrap_or(0);
                    let sessions_done: u32 = parts[5].parse().unwrap_or(0);
                    let is_completed = parts[6].trim() == "Yes";
                    let total_mins = mins * sessions_done;

                    if let Some(entry) = stats_map.iter_mut().find(|e| e.0 == name) {
                        entry.1 += 1;
                        entry.2 += sessions_done;
                        entry.3 += total_mins;
                        if is_completed { entry.4 += 1; } else { entry.5 += 1; }
                    } else {
                        stats_map.push((name, 1, sessions_done, total_mins, if is_completed { 1 } else { 0 }, if is_completed { 0 } else { 1 }));
                    }
                }
            }
        }

        stats_map.iter().map(|(name, entries, sessions, minutes, completed, abandoned)| {
            ActivityStats {
                name: name.clone(),
                total_entries: *entries,
                total_sessions: *sessions,
                total_minutes: *minutes,
                completed: *completed,
                abandoned: *abandoned,
            }
        }).collect()
    }

    fn init_form_for_add(&mut self) {
        self.form_act_idx = 0;
        self.form_mins = self.mins;
        self.form_break_mins = self.break_mins;
        self.form_done = 4;
        self.form_planned = 4;
        self.form_completed = true;
        self.form_date = today_date_digits();
        self.form_cursor = 0;
        self.form_edit_idx = None;
    }

    fn init_form_for_edit(&mut self, display_idx: usize) {
        let raw = self.load_history_raw();
        if display_idx >= raw.len() { return; }
        let line_idx = raw.len() - 1 - display_idx;
        let line = &raw[line_idx];
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() >= 7 {
            self.form_date = parts[0].replace("-", "");
            let act_name = parts[1].to_string();
            self.form_act_idx = self.acts.iter().position(|a| a.replace(",", "") == act_name).unwrap_or(0);
            self.form_mins = parts[2].parse().unwrap_or(25);
            self.form_break_mins = parts[3].parse().unwrap_or(5);
            self.form_planned = parts[4].parse().unwrap_or(4);
            self.form_done = parts[5].parse().unwrap_or(0);
            self.form_completed = parts[6].trim() == "Yes";
        }
        self.form_cursor = 0;
        self.form_edit_idx = Some(display_idx);
    }

    fn load_history_raw(&self) -> Vec<String> {
        let log_path = self.data_dir.join("sessions.csv");
        let mut lines = Vec::new();
        if let Ok(content) = fs::read_to_string(&log_path) {
            for (i, line) in content.lines().enumerate() {
                if i == 0 { continue; }
                lines.push(line.to_string());
            }
        }
        lines
    }

    fn load_history_display(&self) -> Vec<String> {
        let raw = self.load_history_raw();
        let mut display = Vec::new();
        for line in &raw {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 7 {
                display.push(format!(
                    "{} | {} | {}min work / {}min break | {}/{} sessions | {}",
                    parts[0], parts[1], parts[2], parts[3], parts[5], parts[4], parts[6]
                ));
            } else if parts.len() >= 6 {
                display.push(format!(
                    "{} | {} | {}min | {}/{} sessions | {}",
                    parts[0], parts[1], parts[2], parts[4], parts[3], parts[5]
                ));
            }
        }
        display.reverse();
        display
    }

    fn delete_history_entry(&mut self, display_idx: usize) {
        let log_path = self.data_dir.join("sessions.csv");
        if let Ok(content) = fs::read_to_string(&log_path) {
            let all_lines: Vec<&str> = content.lines().collect();
            let data_count = all_lines.len().saturating_sub(1);
            if data_count == 0 || display_idx >= data_count { return; }
            let file_line_idx = data_count - display_idx;
            let mut new_lines: Vec<&str> = Vec::new();
            for (i, line) in all_lines.iter().enumerate() {
                if i != file_line_idx { new_lines.push(line); }
            }
            let _ = fs::write(&log_path, new_lines.join("\n") + "\n");
        }
        let remaining = self.load_history_display().len();
        if remaining == 0 { self.history_scroll = 0; }
        else if self.history_scroll >= remaining { self.history_scroll = remaining - 1; }
    }

    fn start_download(&mut self) {
        let url = self.input.clone();
        let status = self.status_msg.clone();
        let downloading = self.is_downloading.clone();
        let done = self.download_done.clone();
        let target_pattern = self.data_dir.join("bgm").join("%(title)s.%(ext)s");
        let target_str = target_pattern.to_string_lossy().into_owned();
        self.input.clear();
        std::thread::spawn(move || {
            if let Ok(mut d) = downloading.lock() { *d = true; }
            if let Ok(mut s) = status.lock() { *s = "📥 Downloading to system storage...".into(); }
            let cmd = Command::new("yt-dlp")
                .args(["-x", "--audio-format", "mp3", "--quiet", "--no-warnings", "-o", &target_str, &url])
                .stdout(Stdio::null()).stderr(Stdio::null()).status();
            let msg = if let Ok(s) = cmd { if s.success() { "✅ Success! Press ENTER" } else { "❌ Failed" } } else { "❌ yt-dlp missing" };
            if let Ok(mut s) = status.lock() { *s = msg.into(); }
            if let Ok(mut dn) = done.lock() { *dn = true; }
        });
    }

    fn play_bgm(&mut self) {
        self.stop_bgm();
        if self.bgm_idx == 0 { return; }
        if self.bgm_idx >= self.bgm_list.len() { return; }
        let path = self.data_dir.join("bgm").join(&self.bgm_list[self.bgm_idx]);
        if let Ok(file) = fs::File::open(path) {
            if let Ok((stream, handle)) = OutputStream::try_default() {
                if let Ok(sink) = Sink::try_new(&handle) {
                    if let Ok(source) = Decoder::new(io::BufReader::new(file)) {
                        sink.set_volume(if self.muted { 0.0 } else { self.volume });
                        sink.append(source.convert_samples::<f32>().repeat_infinite());
                        if self.paused { sink.pause(); }
                        self.sink = Some(sink);
                        self._stream = Some(stream);
                    }
                }
            }
        }
    }

    fn stop_bgm(&mut self) {
        if let Some(s) = &self.sink { s.stop(); }
        self.sink = None; self._stream = None;
    }

    fn adjust_volume(&mut self, delta: f32) {
        self.volume = (self.volume + delta).clamp(0.0, 1.0);
        if !self.muted { if let Some(s) = &self.sink { s.set_volume(self.volume); } }
    }

    fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        if let Some(s) = &self.sink { s.set_volume(if self.muted { 0.0 } else { self.volume }); }
    }

    fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if let Some(s) = &self.sink { if self.paused { s.pause(); } else { s.play(); } }
    }

    fn on_tick(&mut self) {
        if self.screen != Screen::Timer || self.paused || self.rem == 0 { return; }
        self.rem -= 1;
        self.save_timer_state();
        if self.rem == 0 {
            let (t, b);
            if self.work {
                if self.current >= self.total {
                    t = "Done! 🎉"; b = "All sessions finished!";
                    self.log_session(true);
                    self.clear_timer_state();
                    self.screen = Screen::Activity; self.stop_bgm();
                } else {
                    self.work = false; self.rem = self.break_mins * 60;
                    t = "Break! ☕"; b = "Time to rest.";
                }
            } else {
                self.work = true; self.current += 1; self.rem = self.mins * 60;
                t = "Work! 🔥"; b = "Focus time.";
            }
            if self.notifications_enabled { let _ = notify_rust::Notification::new().summary(t).body(b).show(); }
            self.paused = true; if let Some(s) = &self.sink { s.pause(); }
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    let mut drpc = DiscordIpcClient::new(APP_ID).ok();
    if let Some(ref mut c) = drpc { let _ = c.connect(); }

    let mut app = App::new();
    let mut l_state = ListState::default(); l_state.select(Some(0));

    loop {
        terminal.draw(|f| ui(f, &mut app, &mut l_state))?;
        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press { continue; }
                if *app.is_downloading.lock().unwrap() {
                    if *app.download_done.lock().unwrap() && key.code == KeyCode::Enter {
                        if let Ok(mut d) = app.is_downloading.lock() { *d = false; }
                        if let Ok(mut dn) = app.download_done.lock() { *dn = false; }
                        app.refresh_bgm(); app.screen = Screen::BGM;
                    }
                    continue;
                }
                match app.screen {
                    Screen::Resume => match key.code {
                        KeyCode::Enter | KeyCode::Char('y') => {
                            if app.load_timer_state() {
                                app.paused = true; app.screen = Screen::Timer; app.play_bgm();
                            } else { app.clear_timer_state(); app.screen = Screen::Activity; }
                        }
                        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => {
                            app.clear_timer_state(); app.screen = Screen::Activity;
                        }
                        _ => {}
                    },
                    Screen::Activity => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => { app.idx = app.idx.saturating_sub(1); l_state.select(Some(app.idx)); }
                        KeyCode::Down | KeyCode::Char('j') => { if app.idx < app.menu_len() - 1 { app.idx += 1; l_state.select(Some(app.idx)); } }
                        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                            if app.idx == app.acts.len() {
                                app.history_scroll = 0; app.screen = Screen::History;
                            } else if app.idx == app.acts.len() + 1 {
                                app.stats_scroll = 0; app.screen = Screen::Stats;
                            } else {
                                app.screen = Screen::Duration;
                            }
                        }
                        KeyCode::Char('a') => { app.input.clear(); app.screen = Screen::AddActivity; }
                        KeyCode::Char('e') => {
                            if app.idx >= DEFAULT_ACTS.len() && app.idx < app.acts.len() {
                                app.rename_idx = app.idx;
                                app.input = app.acts[app.idx].clone();
                                app.screen = Screen::RenameActivity;
                            }
                        }
                        KeyCode::Char('d') | KeyCode::Delete => {
                            if app.idx >= DEFAULT_ACTS.len() && app.idx < app.acts.len() {
                                app.acts.remove(app.idx); app.save_config();
                                if app.idx >= app.menu_len() { app.idx = app.menu_len() - 1; }
                                l_state.select(Some(app.idx));
                            }
                        }
                        KeyCode::Char('s') => { app.settings_cursor = 0; app.screen = Screen::Settings; }
                        KeyCode::Char('q') => break,
                        _ => {}
                    },
                    Screen::AddActivity => match key.code {
                        KeyCode::Enter => {
                            let name = app.input.trim().to_string();
                            if !name.is_empty() && !app.acts.contains(&name) { app.acts.push(name); app.save_config(); }
                            app.input.clear(); app.screen = Screen::Activity;
                        }
                        KeyCode::Char(c) => app.input.push(c),
                        KeyCode::Backspace => { app.input.pop(); }
                        KeyCode::Esc => { app.input.clear(); app.screen = Screen::Activity; }
                        _ => {}
                    },
                    Screen::RenameActivity => match key.code {
                        KeyCode::Enter => {
                            let new_name = app.input.trim().to_string();
                            if !new_name.is_empty() && !app.acts.contains(&new_name) {
                                let old_name = app.acts[app.rename_idx].replace(",", "");
                                let clean_new = new_name.replace(",", "");
                                app.acts[app.rename_idx] = new_name;
                                app.rename_activity_in_csv(&old_name, &clean_new);
                                app.save_config();
                            }
                            app.input.clear(); app.screen = Screen::Activity;
                        }
                        KeyCode::Char(c) => app.input.push(c),
                        KeyCode::Backspace => { app.input.pop(); }
                        KeyCode::Esc => { app.input.clear(); app.screen = Screen::Activity; }
                        _ => {}
                    },
                    Screen::Duration => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => app.mins += 1,
                        KeyCode::Down | KeyCode::Char('j') => app.mins = app.mins.saturating_sub(1).max(1),
                        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => app.screen = Screen::Sessions,
                        KeyCode::Char('h') | KeyCode::Left | KeyCode::Esc => app.screen = Screen::Activity,
                        _ => {}
                    },
                    Screen::Sessions => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => app.total += 1,
                        KeyCode::Down | KeyCode::Char('j') => app.total = app.total.saturating_sub(1).max(1),
                        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => { app.screen = Screen::BGM; l_state.select(Some(app.bgm_idx)); }
                        KeyCode::Char('h') | KeyCode::Left | KeyCode::Esc => app.screen = Screen::Duration,
                        _ => {}
                    },
                    Screen::BGM => match key.code {
                        KeyCode::Char('i') => { app.screen = Screen::BGMImport; app.input.clear(); }
                        KeyCode::Up | KeyCode::Char('k') => { app.bgm_idx = app.bgm_idx.saturating_sub(1); l_state.select(Some(app.bgm_idx)); }
                        KeyCode::Down | KeyCode::Char('j') => { if app.bgm_idx < app.bgm_list.len()-1 { app.bgm_idx += 1; l_state.select(Some(app.bgm_idx)); } }
                        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => { app.rem = app.mins * 60; app.current = 1; app.screen = Screen::Timer; app.work = true; app.paused = false; app.session_start = Some(Instant::now()); app.save_timer_state(); app.play_bgm(); }
                        KeyCode::Char('h') | KeyCode::Left | KeyCode::Esc => app.screen = Screen::Sessions,
                        _ => {}
                    },
                    Screen::BGMImport => match key.code {
                        KeyCode::Enter => app.start_download(),
                        KeyCode::Char(c) => app.input.push(c),
                        KeyCode::Backspace => { app.input.pop(); }
                        KeyCode::Esc => app.screen = Screen::BGM,
                        _ => {}
                    },
                    Screen::Settings => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => { if app.settings_cursor > 0 { app.settings_cursor -= 1; } }
                        KeyCode::Down | KeyCode::Char('j') => { if app.settings_cursor < 2 { app.settings_cursor += 1; } }
                        KeyCode::Left | KeyCode::Char('h') => {
                            match app.settings_cursor {
                                0 => app.notifications_enabled = !app.notifications_enabled,
                                1 => app.theme = match app.theme { Theme::Cyan => Theme::Red, Theme::Magenta => Theme::Cyan, Theme::Green => Theme::Magenta, Theme::Yellow => Theme::Green, Theme::Red => Theme::Yellow },
                                2 => app.break_mins = app.break_mins.saturating_sub(1).max(1),
                                _ => {}
                            }
                            app.save_config();
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            match app.settings_cursor {
                                0 => app.notifications_enabled = !app.notifications_enabled,
                                1 => app.theme = match app.theme { Theme::Cyan => Theme::Magenta, Theme::Magenta => Theme::Green, Theme::Green => Theme::Yellow, Theme::Yellow => Theme::Red, Theme::Red => Theme::Cyan },
                                2 => app.break_mins += 1,
                                _ => {}
                            }
                            app.save_config();
                        }
                        KeyCode::Esc | KeyCode::Char('q') => app.screen = Screen::Activity,
                        _ => {}
                    },
                    Screen::Timer => match key.code {
                        KeyCode::Char(' ') => { app.toggle_pause(); app.save_timer_state(); }
                        KeyCode::Char('m') | KeyCode::Char('M') => app.toggle_mute(),
                        KeyCode::Char('+') | KeyCode::Char('=') => { app.adjust_volume(0.05); app.save_config(); }
                        KeyCode::Char('-') | KeyCode::Char('_') => { app.adjust_volume(-0.05); app.save_config(); }
                        KeyCode::Char('q') | KeyCode::Char('h') | KeyCode::Left | KeyCode::Esc => {
                            app.log_session(false); app.clear_timer_state();
                            app.stop_bgm(); app.screen = Screen::Activity;
                        }
                        _ => {}
                    },
                    Screen::History => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => { app.history_scroll = app.history_scroll.saturating_sub(1); }
                        KeyCode::Down | KeyCode::Char('j') => { app.history_scroll += 1; }
                        KeyCode::Char('d') | KeyCode::Delete => {
                            let count = app.load_history_display().len();
                            if count > 0 && app.history_scroll < count { app.delete_history_entry(app.history_scroll); }
                        }
                        KeyCode::Char('e') => {
                            let count = app.load_history_display().len();
                            if count > 0 && app.history_scroll < count {
                                app.init_form_for_edit(app.history_scroll);
                                app.screen = Screen::SessionForm;
                            }
                        }
                        KeyCode::Char('a') => {
                            app.init_form_for_add();
                            app.screen = Screen::SessionForm;
                        }
                        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Left => app.screen = Screen::Activity,
                        _ => {}
                    },
                    Screen::SessionForm => {
                        let num_fields = 7;
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k') => { if app.form_cursor > 0 { app.form_cursor -= 1; } }
                            KeyCode::Down | KeyCode::Char('j') => { if app.form_cursor < num_fields - 1 { app.form_cursor += 1; } }
                            KeyCode::Right | KeyCode::Char('l') => {
                                match app.form_cursor {
                                    0 => {} // date: type only
                                    1 => { if app.form_act_idx < app.acts.len() - 1 { app.form_act_idx += 1; } }
                                    2 => app.form_mins += 1,
                                    3 => app.form_break_mins += 1,
                                    4 => app.form_done += 1,
                                    5 => app.form_planned += 1,
                                    6 => app.form_completed = !app.form_completed,
                                    _ => {}
                                }
                            }
                            KeyCode::Left | KeyCode::Char('h') => {
                                match app.form_cursor {
                                    0 => {} // date: type only
                                    1 => { app.form_act_idx = app.form_act_idx.saturating_sub(1); }
                                    2 => app.form_mins = app.form_mins.saturating_sub(1).max(1),
                                    3 => app.form_break_mins = app.form_break_mins.saturating_sub(1).max(1),
                                    4 => app.form_done = app.form_done.saturating_sub(1),
                                    5 => app.form_planned = app.form_planned.saturating_sub(1).max(1),
                                    6 => app.form_completed = !app.form_completed,
                                    _ => {}
                                }
                            }
                            KeyCode::Char(c) if app.form_cursor == 0 && c.is_ascii_digit() && app.form_date.len() < 8 => {
                                app.form_date.push(c);
                            }
                            KeyCode::Backspace if app.form_cursor == 0 => {
                                app.form_date.pop();
                            }
                            KeyCode::Enter => {
                                if let Some(edit_idx) = app.form_edit_idx {
                                    app.update_session_entry(edit_idx);
                                } else {
                                    app.log_manual_session();
                                }
                                app.screen = Screen::History;
                            }
                            KeyCode::Esc | KeyCode::Char('q') => { app.screen = Screen::History; }
                            _ => {}
                        }
                    },
                    Screen::Stats => match key.code {
                        KeyCode::Up | KeyCode::Char('k') => { app.stats_scroll = app.stats_scroll.saturating_sub(1); }
                        KeyCode::Down | KeyCode::Char('j') => { app.stats_scroll += 1; }
                        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Left => app.screen = Screen::Activity,
                        _ => {}
                    },
                }
            }
        }
        if app.tick.elapsed() >= Duration::from_secs(1) {
            app.on_tick(); update_presence(&mut drpc, &app); app.tick = Instant::now();
        }
    }
    disable_raw_mode()?; execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?; Ok(())
}

fn ui(f: &mut ratatui::Frame, app: &mut App, l_state: &mut ListState) {
    let size = f.size();
    let theme_color = app.theme.color();
    let chunks = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(3), Constraint::Min(10), Constraint::Length(3)]).split(size);

    f.render_widget(Paragraph::new("POMODORO TUI").alignment(Alignment::Center).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme_color))), chunks[0]);
    let main_area = centered_rect(70, 60, chunks[1]);

    if *app.is_downloading.lock().unwrap() {
        let msg = app.status_msg.lock().unwrap().clone();
        f.render_widget(Paragraph::new(format!("\n\n{}", msg)).alignment(Alignment::Center).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme_color)).title(" Background Process ")), main_area);
    } else {
        match app.screen {
            Screen::Resume => {
                let mut info = String::from("\n\n⏸️ Previous session found!\n\n");
                if let Ok(content) = fs::read_to_string(app.state_path()) {
                    let mut s_current = 0u32; let mut s_total = 0u32; let mut s_rem = 0u32;
                    let mut s_idx = 0usize; let mut s_work = true;
                    for line in content.lines() {
                        let parts: Vec<&str> = line.splitn(2, '=').collect();
                        if parts.len() != 2 { continue; }
                        match parts[0].trim() {
                            "current" => { s_current = parts[1].trim().parse().unwrap_or(0); }
                            "total" => { s_total = parts[1].trim().parse().unwrap_or(0); }
                            "rem" => { s_rem = parts[1].trim().parse().unwrap_or(0); }
                            "idx" => { s_idx = parts[1].trim().parse().unwrap_or(0); }
                            "work" => { s_work = parts[1].trim() == "true"; }
                            _ => {}
                        }
                    }
                    let act_name = if s_idx < app.acts.len() { &app.acts[s_idx] } else { "Unknown" };
                    let phase = if s_work { "Focus" } else { "Break" };
                    info.push_str(&format!("{} | {} | Session {} of {}\n{}:{:02} remaining", act_name, phase, s_current, s_total, s_rem / 60, s_rem % 60));
                } else { info.push_str("Could not read session details."); }
                f.render_widget(Paragraph::new(info).alignment(Alignment::Center).block(Block::default().title(" Resume Session? ").borders(Borders::ALL).border_style(Style::default().fg(theme_color))), main_area);
            }
            Screen::Activity => {
                let mut items: Vec<ListItem> = app.acts.iter().map(|a| ListItem::new(a.as_str())).collect();
                items.push(ListItem::new("Previous Sessions 📋"));
                items.push(ListItem::new("Session Stats 📊"));
                f.render_stateful_widget(List::new(items).block(Block::default().title(" [1] Select Activity ").borders(Borders::ALL).border_style(Style::default().fg(theme_color))).highlight_style(Style::default().bg(theme_color).fg(Color::Black)), main_area, l_state);
            }
            Screen::AddActivity => {
                f.render_widget(Paragraph::new(format!("\nActivity Name:\n{}\n\n[Enter] Add | [Esc] Cancel", app.input)).alignment(Alignment::Center).block(Block::default().title(" Add Custom Activity ").borders(Borders::ALL).border_style(Style::default().fg(theme_color))), main_area);
            }
            Screen::RenameActivity => {
                f.render_widget(Paragraph::new(format!("\nNew Name:\n{}\n\n[Enter] Save | [Esc] Cancel\nAll history entries will be updated.", app.input)).alignment(Alignment::Center).block(Block::default().title(" Rename Activity ").borders(Borders::ALL).border_style(Style::default().fg(theme_color))), main_area);
            }
            Screen::Duration => {
                f.render_widget(Paragraph::new(format!("\n\nFocus Time: {} min\n\n[J/K] Adjust | [L/Right] Next", app.mins)).alignment(Alignment::Center).block(Block::default().title(" [2] Duration ").borders(Borders::ALL).border_style(Style::default().fg(theme_color))), main_area);
            }
            Screen::Sessions => {
                f.render_widget(Paragraph::new(format!("\n\nSessions: {}\n\n[J/K] Adjust | [L/Right] Next", app.total)).alignment(Alignment::Center).block(Block::default().title(" [3] Sessions ").borders(Borders::ALL).border_style(Style::default().fg(theme_color))), main_area);
            }
            Screen::BGM => {
                let items: Vec<ListItem> = app.bgm_list.iter().map(|b| ListItem::new(b.as_str())).collect();
                f.render_stateful_widget(List::new(items).block(Block::default().title(" [4] Background Song (Press 'i' to Import) ").borders(Borders::ALL).border_style(Style::default().fg(theme_color))).highlight_style(Style::default().bg(theme_color).fg(Color::Black)), main_area, l_state);
            }
            Screen::BGMImport => {
                f.render_widget(Paragraph::new(format!("\nPaste YouTube URL:\n{}\n\n[Enter] Download | [Esc] Cancel", app.input)).alignment(Alignment::Center).block(Block::default().title(" Import BGM ").borders(Borders::ALL).border_style(Style::default().fg(theme_color))), main_area);
            }
            Screen::Settings => {
                let n_status = if app.notifications_enabled { "ON" } else { "OFF" };
                let t_name = format!("{:?}", app.theme);
                let text = vec![
                    Line::from(vec![Span::styled(if app.settings_cursor == 0 { "> Notifications: " } else { "  Notifications: " }, Style::default().fg(if app.settings_cursor == 0 { theme_color } else { Color::White })), Span::raw(n_status)]),
                    Line::from(vec![Span::raw("")]),
                    Line::from(vec![Span::styled(if app.settings_cursor == 1 { "> Theme: " } else { "  Theme: " }, Style::default().fg(if app.settings_cursor == 1 { theme_color } else { Color::White })), Span::raw(&t_name)]),
                    Line::from(vec![Span::raw("")]),
                    Line::from(vec![Span::styled(if app.settings_cursor == 2 { "> Break Duration: " } else { "  Break Duration: " }, Style::default().fg(if app.settings_cursor == 2 { theme_color } else { Color::White })), Span::raw(format!("{} min", app.break_mins))]),
                ];
                f.render_widget(Paragraph::new(text).alignment(Alignment::Center).block(Block::default().title(" Settings ").borders(Borders::ALL).border_style(Style::default().fg(theme_color))), main_area);
            }
            Screen::Timer => {
                let total = if app.work { app.mins * 60 } else { app.break_mins * 60 };
                let pct = if total > 0 { ((total - app.rem.min(total)) as f64 / total as f64 * 100.0) as u16 } else { 0 };
                let gauge_color = if app.paused { Color::Gray } else if app.work { Color::Red } else { Color::Green };
                let v_level = if app.muted { "Muted".to_string() } else { format!("{}%", (app.volume * 100.0) as u32) };
                let phase = if app.paused { "⏸ PAUSED" } else if app.work { "🔥 FOCUS" } else { "☕ BREAK" };
                let label_text = format!("{} | {}:{:02} | Vol: {}", phase, app.rem / 60, app.rem % 60, v_level);
                f.render_widget(
                    Gauge::default()
                        .block(Block::default().title(format!(" Session {} of {} ", app.current, app.total)).borders(Borders::ALL))
                        .gauge_style(Style::default().fg(gauge_color).bg(Color::DarkGray))
                        .label(Span::styled(label_text, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)))
                        .percent(pct.min(100)),
                    main_area,
                );
            }
            Screen::History => {
                let history = app.load_history_display();
                let has_entries = !history.is_empty();
                let display: Vec<ListItem> = if has_entries {
                    history.iter().map(|s| ListItem::new(s.as_str())).collect()
                } else { vec![ListItem::new("No sessions recorded yet.")] };
                let max_scroll = display.len().saturating_sub(1);
                if app.history_scroll > max_scroll { app.history_scroll = max_scroll; }
                let mut state = ListState::default();
                state.select(Some(app.history_scroll));
                f.render_stateful_widget(
                    List::new(display).block(Block::default().title(" [5] Previous Sessions ").borders(Borders::ALL).border_style(Style::default().fg(theme_color)))
                        .highlight_style(Style::default().bg(theme_color).fg(Color::Black)),
                    main_area, &mut state,
                );
            }
            Screen::SessionForm => {
                let title = if app.form_edit_idx.is_some() { " Edit Session " } else { " Add Session " };
                let act_name = if app.form_act_idx < app.acts.len() { &app.acts[app.form_act_idx] } else { "?" };
                let fields = vec![
                    ("Date", format!("{}", format_date_display(&app.form_date))),
                    ("Activity", format!("< {} >", act_name)),
                    ("Duration", format!("< {} min >", app.form_mins)),
                    ("Break", format!("< {} min >", app.form_break_mins)),
                    ("Sessions Done", format!("< {} >", app.form_done)),
                    ("Sessions Planned", format!("< {} >", app.form_planned)),
                    ("Completed", format!("< {} >", if app.form_completed { "Yes" } else { "No" })),
                ];
                let text: Vec<Line> = fields.iter().enumerate().flat_map(|(i, (label, val))| {
                    let prefix = if i == app.form_cursor { "> " } else { "  " };
                    let color = if i == app.form_cursor { theme_color } else { Color::White };
                    vec![Line::from(vec![Span::styled(format!("{}{}: ", prefix, label), Style::default().fg(color)), Span::raw(val)])]
                }).collect();
                f.render_widget(Paragraph::new(text).block(Block::default().title(title).borders(Borders::ALL).border_style(Style::default().fg(theme_color))), main_area);
            }
            Screen::Stats => {
                let stats = app.load_stats();
                if stats.is_empty() {
                    f.render_widget(Paragraph::new("\n\nNo session data yet.").alignment(Alignment::Center).block(Block::default().title(" [6] Session Stats ").borders(Borders::ALL).border_style(Style::default().fg(theme_color))), main_area);
                } else {
                    let items: Vec<ListItem> = stats.iter().map(|s| {
                        let hours = s.total_minutes / 60;
                        let mins = s.total_minutes % 60;
                        let time_str = if hours > 0 { format!("{}h {}min", hours, mins) } else { format!("{}min", mins) };
                        let avg = if s.total_entries > 0 { s.total_sessions as f64 / s.total_entries as f64 } else { 0.0 };
                        ListItem::new(format!(
                            "{} | {} sessions ({} entries, {:.1} avg) | {} | {} done, {} quit",
                            s.name, s.total_sessions, s.total_entries, avg, time_str, s.completed, s.abandoned
                        ))
                    }).collect();
                    let max_scroll = items.len().saturating_sub(1);
                    if app.stats_scroll > max_scroll { app.stats_scroll = max_scroll; }
                    let mut state = ListState::default();
                    state.select(Some(app.stats_scroll));
                    f.render_stateful_widget(
                        List::new(items).block(Block::default().title(" [6] Session Stats ").borders(Borders::ALL).border_style(Style::default().fg(theme_color)))
                            .highlight_style(Style::default().bg(theme_color).fg(Color::Black)),
                        main_area, &mut state,
                    );
                }
            }
        }
    }
    
    let help_text = match app.screen {
        Screen::Resume => " [Enter/Y] Resume | [Esc/N] Discard ",
        Screen::Activity => " [J/K] Move | [A] Add | [E] Rename | [D] Delete | [S] Settings | [Q] Quit ",
        Screen::AddActivity | Screen::RenameActivity => " [Enter] Save | [Esc] Cancel ",
        Screen::Timer => " [Space] Pause | [+/-] Vol | [M] Mute | [H/Left] Stop & Menu ",
        Screen::Settings => " [J/K] Select | [H/L] Change | [Esc/Q] Back ",
        Screen::History => " [J/K] Scroll | [A] Add | [E] Edit | [D] Delete | [Esc/Q] Back ",
        Screen::SessionForm => " [J/K] Select Field | [H/L] Adjust | [Enter] Save | [Esc] Cancel ",
        Screen::Stats => " [J/K] Scroll | [Esc/Q/Left] Back ",
        _ => " [Arrows/HJKL] Navigate | [Esc] Back ",
    };
    f.render_widget(Paragraph::new(help_text).alignment(Alignment::Center).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray))), chunks[2]);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default().direction(Direction::Vertical).constraints([Constraint::Percentage((100 - percent_y) / 2), Constraint::Percentage(percent_y), Constraint::Percentage((100 - percent_y) / 2)]).split(r);
    Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage((100 - percent_x) / 2), Constraint::Percentage(percent_x), Constraint::Percentage((100 - percent_x) / 2)]).split(popup_layout[1])[1]
}

fn update_presence(drpc: &mut Option<DiscordIpcClient>, app: &App) {
    if let Some(c) = drpc {
        let (state, details) = match app.screen {
            Screen::Timer => (if app.paused { format!("⏸️ Paused: {}", app.acts[app.idx]) } else if !app.work { "☕ Taking a Break".to_string() } else { format!("🔥 Focusing: {}", app.acts[app.idx]) }, format!("Session {} of {}", app.current, app.total)),
            _ => ("Configuring...".into(), "Main Menu".into()),
        };
        let mut p = activity::Activity::new().state(&state).details(&details).assets(activity::Assets::new().large_image("app_icon"));
        if app.screen == Screen::Timer && !app.paused {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
            p = p.timestamps(activity::Timestamps::new().end((now + app.rem as u64) as i64));
        }
        let _ = c.set_activity(p);
    }
}
