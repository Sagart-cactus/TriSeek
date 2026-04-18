use search_core::IndexMetadata;
use search_index::{BuildPhase, BuildProgress, BuildProgressSnapshot};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const DEFAULT_TERMINAL_COLS: usize = 120;
const MIN_TERMINAL_COLS: usize = 72;
const LABEL_WIDTH: usize = 16;
const MIN_VALUE_WIDTH: usize = 28;
const PROGRESS_BAR_WIDTH: usize = 28;
const SPINNER_FRAMES: [&str; 4] = ["⠋", "⠙", "⠹", "⠸"];

pub fn render_build_summary(
    index_dir: &Path,
    metadata: &IndexMetadata,
    terminal_cols: Option<usize>,
) -> String {
    let max_width = terminal_cols
        .unwrap_or(DEFAULT_TERMINAL_COLS)
        .max(MIN_TERMINAL_COLS);
    let value_width = max_width
        .saturating_sub(table_overhead(2) + LABEL_WIDTH)
        .max(MIN_VALUE_WIDTH);

    let mut rows = vec![
        ("Repo", metadata.repo_stats.repo_name.clone()),
        (
            "Root",
            truncate_middle(&metadata.repo_stats.repo_root, value_width),
        ),
        (
            "Index",
            truncate_middle(&index_dir.display().to_string(), value_width),
        ),
        (
            "Files",
            format!(
                "{} indexed, {} skipped, {} tracked",
                format_number(metadata.build_stats.docs_indexed),
                format_number(metadata.build_stats.files_skipped),
                format_number(metadata.repo_stats.tracked_files)
            ),
        ),
        (
            "Searchable",
            format!(
                "{} across {} files",
                format_bytes(metadata.repo_stats.searchable_bytes),
                format_number(metadata.repo_stats.searchable_files)
            ),
        ),
        ("Index Size", format_bytes(metadata.build_stats.index_bytes)),
        (
            "Postings",
            format_number(metadata.build_stats.total_postings),
        ),
        (
            "Build Time",
            format_duration(metadata.build_stats.build_millis),
        ),
        ("Updated", metadata.build_stats.completed_at.clone()),
    ];

    let languages = summarize_languages(&metadata.repo_stats.languages, value_width);
    if !languages.is_empty() {
        rows.push(("Languages", languages));
    }

    render_kv_table("Index Build Complete", &rows, value_width)
}

pub struct BuildProgressRenderer {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl BuildProgressRenderer {
    pub fn spawn(progress: BuildProgress) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = stop.clone();
        let handle = thread::spawn(move || progress_loop(progress, stop_flag));
        Self {
            stop,
            handle: Some(handle),
        }
    }

    pub fn finish(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn progress_loop(progress: BuildProgress, stop: Arc<AtomicBool>) {
    let stderr = io::stderr();
    let mut stream = stderr.lock();
    let mut frame = 0usize;
    let mut last_width = 0usize;
    let mut last_line = String::new();

    loop {
        if stop.load(Ordering::Relaxed) {
            clear_status_line(&mut stream, last_width);
            break;
        }

        let snapshot = progress.snapshot();
        let line = render_progress_line(snapshot, frame);
        if line != last_line {
            last_width = write_status_line(&mut stream, &line, last_width);
            last_line = line;
        }
        frame = frame.wrapping_add(1);
        thread::sleep(Duration::from_millis(100));
    }
}

fn render_progress_line(snapshot: BuildProgressSnapshot, frame: usize) -> String {
    match snapshot.phase {
        BuildPhase::Scanning | BuildPhase::Idle => format!(
            "{} Scanning files: {} seen, {} indexable, {}",
            SPINNER_FRAMES[frame % SPINNER_FRAMES.len()],
            format_number(snapshot.tracked_files),
            format_number(snapshot.searchable_files),
            format_bytes(snapshot.searchable_bytes)
        ),
        BuildPhase::Indexing => render_indexing_line(snapshot),
        BuildPhase::Persisting => format!(
            "{} Writing index files...",
            SPINNER_FRAMES[frame % SPINNER_FRAMES.len()]
        ),
        BuildPhase::Finished => "✔ Index build complete".to_string(),
    }
}

fn render_indexing_line(snapshot: BuildProgressSnapshot) -> String {
    let total_units = snapshot.total_bytes.max(1);
    let completed_units = snapshot.indexed_bytes.min(total_units);
    let filled = ((completed_units as usize * PROGRESS_BAR_WIDTH) / total_units as usize)
        .min(PROGRESS_BAR_WIDTH);
    let empty = PROGRESS_BAR_WIDTH.saturating_sub(filled);
    let percent = ((completed_units as f64 / total_units as f64) * 100.0).min(100.0);

    format!(
        "Indexing [{}{}] {:>5.1}% {}/{} files {} / {}",
        "█".repeat(filled),
        "░".repeat(empty),
        percent,
        format_number(snapshot.indexed_files),
        format_number(snapshot.total_files),
        format_bytes(snapshot.indexed_bytes),
        format_bytes(snapshot.total_bytes)
    )
}

fn write_status_line(stream: &mut dyn Write, line: &str, last_width: usize) -> usize {
    let line_width = display_width(line);
    let clear_width = last_width.saturating_sub(line_width);
    let _ = write!(stream, "\r{line}{}", " ".repeat(clear_width));
    let _ = stream.flush();
    line_width
}

fn clear_status_line(stream: &mut dyn Write, last_width: usize) {
    let _ = write!(stream, "\r{}\r", " ".repeat(last_width));
    let _ = stream.flush();
}

fn render_kv_table(title: &str, rows: &[(&str, String)], value_width: usize) -> String {
    let mut out = String::new();
    out.push_str(title);
    out.push('\n');

    let widths = [LABEL_WIDTH, value_width];
    write_border(&mut out, '┌', '┬', '┐', &widths);
    write_row(
        &mut out,
        &[
            ("Field", LABEL_WIDTH, Align::Center),
            ("Value", value_width, Align::Center),
        ],
    );
    write_border(&mut out, '├', '┼', '┤', &widths);

    for (index, (label, value)) in rows.iter().enumerate() {
        write_row(
            &mut out,
            &[
                (label, LABEL_WIDTH, Align::Left),
                (value.as_str(), value_width, Align::Left),
            ],
        );
        let (left, mid, right) = if index + 1 == rows.len() {
            ('└', '┴', '┘')
        } else {
            ('├', '┼', '┤')
        };
        write_border(&mut out, left, mid, right, &widths);
    }

    out
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Center,
}

fn write_border(out: &mut String, left: char, mid: char, right: char, widths: &[usize]) {
    out.push(left);
    for (index, width) in widths.iter().enumerate() {
        for _ in 0..(width + 2) {
            out.push('─');
        }
        out.push(if index + 1 == widths.len() {
            right
        } else {
            mid
        });
    }
    out.push('\n');
}

fn write_row(out: &mut String, cells: &[(&str, usize, Align)]) {
    out.push('│');
    for (content, width, align) in cells {
        out.push(' ');
        write_aligned(out, content, *width, *align);
        out.push(' ');
        out.push('│');
    }
    out.push('\n');
}

fn write_aligned(out: &mut String, content: &str, width: usize, align: Align) {
    let text_width = display_width(content);
    let padding = width.saturating_sub(text_width);
    let (left_pad, right_pad) = match align {
        Align::Left => (0, padding),
        Align::Center => (padding / 2, padding - (padding / 2)),
    };

    for _ in 0..left_pad {
        out.push(' ');
    }
    out.push_str(content);
    for _ in 0..right_pad {
        out.push(' ');
    }
}

fn table_overhead(columns: usize) -> usize {
    (columns * 3) + 1
}

fn display_width(text: &str) -> usize {
    text.chars().count()
}

fn truncate_middle(text: &str, max_chars: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_chars {
        return text.to_string();
    }
    if max_chars <= 1 {
        return "…".to_string();
    }

    let left = (max_chars - 1) / 2;
    let right = max_chars - 1 - left;
    let mut truncated = String::new();
    for ch in &chars[..left] {
        truncated.push(*ch);
    }
    truncated.push('…');
    for ch in &chars[chars.len() - right..] {
        truncated.push(*ch);
    }
    truncated
}

fn summarize_languages(languages: &[(String, u64)], max_chars: usize) -> String {
    let summary = languages
        .iter()
        .take(5)
        .map(|(language, count)| format!("{language} {}", format_number(*count)))
        .collect::<Vec<_>>()
        .join(", ");
    truncate_middle(&summary, max_chars)
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn format_duration(millis: u128) -> String {
    if millis >= 1_000 {
        format!("{:.2} s", millis as f64 / 1_000.0)
    } else {
        format!("{millis} ms")
    }
}

fn format_number(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use search_core::{BuildStats, IndexMetadata, RepoStats};

    #[test]
    fn build_summary_is_compact_and_human_readable() {
        let metadata = IndexMetadata {
            schema_version: 1,
            repo_stats: RepoStats {
                repo_name: "TriSeek".to_string(),
                repo_root: "/Users/trivedi/Documents/Projects/TriSeek".to_string(),
                tracked_files: 420,
                searchable_files: 398,
                searchable_bytes: 12_345_678,
                languages: vec![
                    ("rs".to_string(), 210),
                    ("md".to_string(), 32),
                    ("toml".to_string(), 11),
                ],
                ..RepoStats::default()
            },
            build_stats: BuildStats {
                completed_at: "2026-04-18T08:00:00Z".to_string(),
                docs_indexed: 398,
                files_skipped: 22,
                total_postings: 9_876_543,
                index_bytes: 3_456_789,
                build_millis: 2_345,
                update_millis: None,
            },
            delta_docs: 0,
            delta_removed_paths: 0,
        };

        let rendered = render_build_summary(Path::new("/tmp/index"), &metadata, Some(100));
        assert!(rendered.contains("Index Build Complete"));
        assert!(rendered.contains("398 indexed, 22 skipped, 420 tracked"));
        assert!(rendered.contains("11.8 MiB across 398 files"));
        assert!(rendered.contains("3.3 MiB"));
        assert!(rendered.contains("9,876,543"));
        assert!(rendered.contains("2.35 s"));
        assert!(rendered.contains("rs 210, md 32, toml 11"));
        assert!(!rendered.contains("\"schema_version\""));
    }
}
