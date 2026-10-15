use crate::app_context::AppCategory;
use crate::errors::{CoreError, Result};
use std::path::Path;

/// Starter list of IT terms and brands with spoken Russian variants, shipped
/// with the app and copied to the config dir on first run.
pub const STARTER_FILE_NAME: &str = "dictionary-it-ru.txt";
pub const STARTER_DICTIONARY: &str = include_str!("../resources/dictionary-it-ru.txt");

/// Most entries in the Omni instructions block.
pub const MAX_PROMPT_ENTRIES: usize = 120;
/// Most characters of entries in the Omni instructions block.
pub const MAX_PROMPT_CHARS: usize = 2500;
/// Spoken variants shown per starter entry.
const MAX_STARTER_VARIANTS: usize = 2;
/// Spoken variants shown per user entry.
const MAX_USER_VARIANTS: usize = 3;
/// Share of the character budget the app-specific sections may take before the
/// general core gets its turn.
const SPECIFIC_SHARE_PERCENT: usize = 70;

/// One dictionary line: `Correct | spoken1, spoken2`, or just `Correct`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictEntry {
    pub term: String,
    pub variants: Vec<String>,
}

impl DictEntry {
    pub fn parse(line: &str) -> Option<Self> {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (term, variants) = match line.split_once('|') {
            Some((term, variants)) => (term.trim(), variants),
            None => (line, ""),
        };
        if term.is_empty() {
            return None;
        }
        let variants = variants
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(String::from)
            .collect();
        Some(Self {
            term: term.to_string(),
            variants,
        })
    }

    fn render(&self, max_variants: usize) -> String {
        if self.variants.is_empty() || max_variants == 0 {
            return self.term.clone();
        }
        let shown: Vec<&str> = self
            .variants
            .iter()
            .take(max_variants)
            .map(String::as_str)
            .collect();
        format!("{} ({})", self.term, shown.join(", "))
    }
}

/// A `# …` section of the starter list with its entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    pub entries: Vec<DictEntry>,
}

/// Dictionary entries, deduplicated by term, order kept. Accepts both plain
/// lines and `Correct | variants`.
pub fn parse_entries(content: &str) -> Vec<DictEntry> {
    let mut seen = std::collections::HashSet::new();
    content
        .lines()
        .filter_map(DictEntry::parse)
        .filter(|e| seen.insert(e.term.clone()))
        .collect()
}

/// Sections of the starter list. A run of comment lines directly above entries
/// starts a section, the first line of the run is its title; a comment run with
/// no entries after it (the file header) starts nothing.
pub fn parse_sections(content: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    let mut title: Option<String> = None;
    let mut in_comment_run = false;
    for line in content.lines().map(str::trim) {
        if let Some(comment) = line.strip_prefix('#') {
            if !in_comment_run {
                title = Some(comment.trim().to_string());
            }
            in_comment_run = true;
        } else if line.is_empty() {
            in_comment_run = false;
        } else if let Some(entry) = DictEntry::parse(line) {
            in_comment_run = false;
            match sections.last_mut() {
                Some(section) if title.is_none() => section.entries.push(entry),
                _ => {
                    sections.push(Section {
                        title: title.take().unwrap_or_default(),
                        entries: vec![entry],
                    });
                }
            }
            title = None;
        }
    }
    sections
}

/// Which app categories a starter section is most useful for, by its title.
fn section_is_for(title: &str, category: AppCategory) -> bool {
    let title = title.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| title.contains(w));
    match category {
        AppCategory::Dev => has(&[
            "git",
            "инфраструктур",
            "редактор",
            "язык",
            "фреймворк",
            "баз данных",
            "протокол",
            "командн",
            "ии и",
        ]),
        AppCategory::Messenger => has(&["мессенджер", "российск", "аббревиатур", "apple", "ии и"]),
        AppCategory::Mail => has(&["аббревиатур", "российск", "мессенджер", "редактор"]),
        AppCategory::Browser => has(&["ии и", "редактор", "российск", "мессенджер", "git"]),
        AppCategory::Other => false,
    }
}

fn is_homonym_section(title: &str) -> bool {
    title.to_lowercase().contains("омоним")
}

/// Round-robin over `pools`, taking one entry from each in turn, so that no
/// single section eats the budget.
fn interleave<'a>(pools: &[&'a Section]) -> Vec<&'a DictEntry> {
    let longest = pools.iter().map(|s| s.entries.len()).max().unwrap_or(0);
    let mut out = Vec::new();
    for i in 0..longest {
        for section in pools {
            if let Some(entry) = section.entries.get(i) {
                out.push(entry);
            }
        }
    }
    out
}

struct Budget {
    entries: usize,
    chars: usize,
}

impl Budget {
    fn new() -> Self {
        Self {
            entries: 0,
            chars: 0,
        }
    }

    /// Adds `text` if it still fits; the `"; "` separator counts.
    fn take(&mut self, text: &str) -> bool {
        let cost = text.chars().count() + if self.entries == 0 { 0 } else { 2 };
        if self.entries >= MAX_PROMPT_ENTRIES || self.chars + cost > MAX_PROMPT_CHARS {
            return false;
        }
        self.entries += 1;
        self.chars += cost;
        true
    }
}

/// User entries rendered for the instructions block, in order, as far as the
/// budget allows. `seen` collects their lowercased terms.
fn take_user_entries(
    user: &[DictEntry],
    budget: &mut Budget,
    seen: &mut std::collections::HashSet<String>,
) -> Vec<String> {
    let mut items = Vec::new();
    for entry in user {
        if seen.insert(entry.term.to_lowercase()) {
            let text = entry.render(MAX_USER_VARIANTS);
            if budget.take(&text) {
                items.push(text);
            }
        }
    }
    items
}

/// How much of the user's dictionary reaches the Omni instructions block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserBudget {
    /// Distinct terms (case-insensitive).
    pub entries: usize,
    /// Terms that fit under `MAX_PROMPT_ENTRIES` and `MAX_PROMPT_CHARS`.
    pub kept: usize,
    /// Characters the kept terms take.
    pub chars: usize,
}

/// The share of the instructions block the user's own entries take; the
/// starter list only fills what they leave.
pub fn user_budget(user: &[DictEntry]) -> UserBudget {
    let mut budget = Budget::new();
    let mut seen = std::collections::HashSet::new();
    let kept = take_user_entries(user, &mut budget, &mut seen).len();
    UserBudget {
        entries: seen.len(),
        kept,
        chars: budget.chars,
    }
}

/// The compact "correct spelling" block for the Omni session instructions, or
/// `None` when there is nothing to say. User entries go first and always win;
/// then starter sections matching the frontmost app's category, then the
/// general core. Homonym entries (terms that sound like ordinary Russian words)
/// are included only for developer tools, last, as bare terms.
pub fn build_prompt_block(
    user: &[DictEntry],
    starter: &[Section],
    category: AppCategory,
    developer_tool: bool,
) -> Option<String> {
    let mut budget = Budget::new();
    let mut seen = std::collections::HashSet::new();
    let mut items = take_user_entries(user, &mut budget, &mut seen);

    let regular: Vec<&Section> = starter
        .iter()
        .filter(|s| !is_homonym_section(&s.title))
        .collect();
    let (specific, core): (Vec<&Section>, Vec<&Section>) = regular
        .into_iter()
        .partition(|s| section_is_for(&s.title, category));

    let specific_limit = MAX_PROMPT_CHARS * SPECIFIC_SHARE_PERCENT / 100;
    'specific: for entry in interleave(&specific) {
        if !seen.insert(entry.term.to_lowercase()) {
            continue;
        }
        let text = entry.render(MAX_STARTER_VARIANTS);
        if budget.chars + text.chars().count() > specific_limit {
            break 'specific;
        }
        if budget.take(&text) {
            items.push(text);
        }
    }
    for entry in interleave(&core) {
        if !seen.insert(entry.term.to_lowercase()) {
            continue;
        }
        let text = entry.render(MAX_STARTER_VARIANTS);
        if budget.take(&text) {
            items.push(text);
        }
    }

    let mut homonyms: Vec<String> = Vec::new();
    if developer_tool {
        for section in starter.iter().filter(|s| is_homonym_section(&s.title)) {
            for entry in &section.entries {
                if seen.insert(entry.term.to_lowercase()) && budget.take(&entry.term) {
                    homonyms.push(entry.term.clone());
                }
            }
        }
    }

    if items.is_empty() && homonyms.is_empty() {
        return None;
    }
    let mut block = String::new();
    if !items.is_empty() {
        block.push_str(
            "Правильное написание терминов (если услышишь похожее по звучанию — пиши так): ",
        );
        block.push_str(&items.join("; "));
        block.push('.');
    }
    if !homonyms.is_empty() {
        if !block.is_empty() {
            block.push(' ');
        }
        block.push_str(
            "Названия программ и языков, которые звучат как обычные слова (пиши латиницей, \
             только если по смыслу речь о программе, языке или инструменте): ",
        );
        block.push_str(&homonyms.join(", "));
        block.push('.');
    }
    Some(block)
}

/// Load dictionary terms from a text file (the correct spelling of each line).
/// Skips empty lines and lines starting with '#'; for `Correct | variants`
/// lines only `Correct` is returned. Deduplicated, order preserved.
pub fn load_dictionary(path: &Path) -> Result<Vec<String>> {
    Ok(load_entries(path)?.into_iter().map(|e| e.term).collect())
}

/// Load dictionary entries, with spoken variants where the file has them.
pub fn load_entries(path: &Path) -> Result<Vec<DictEntry>> {
    if !path.exists() {
        log::info!("dictionary file not found: {}, using empty", path.display());
        return Ok(vec![]);
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| CoreError::Config(format!("read {}: {e}", path.display())))?;
    let entries = parse_entries(&content);

    log::info!(
        "loaded {} dictionary entries from {}",
        entries.len(),
        path.display()
    );
    Ok(entries)
}

/// Sections of the starter list file, falling back to the built-in copy when
/// the file is missing or unreadable.
pub fn load_starter_sections(path: &Path) -> Vec<Section> {
    match std::fs::read_to_string(path) {
        Ok(content) => parse_sections(&content),
        Err(_) => parse_sections(STARTER_DICTIONARY),
    }
}

/// Write the starter list next to the config unless it is already there.
pub fn ensure_starter_file(dir: &Path) -> Result<bool> {
    let path = dir.join(STARTER_FILE_NAME);
    if path.exists() {
        return Ok(false);
    }
    std::fs::write(&path, STARTER_DICTIONARY)
        .map_err(|e| CoreError::Config(format!("write {}: {e}", path.display())))?;
    log::info!("created default: {}", path.display());
    Ok(true)
}

/// How [`add_entry`] changed a dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddOutcome {
    /// A new line was appended.
    Added,
    /// An existing entry got new variants or a new spelling of its term.
    Updated,
    /// The entry and all the variants were already there.
    Unchanged,
}

/// Lowercased Cyrillic-only form of `heard`, or None when it is not a spoken
/// variant worth keeping (Latin, digits, empty).
fn spoken_variant(heard: &str) -> Option<String> {
    let heard = heard.trim();
    let is_spoken = !heard.is_empty()
        && heard.chars().any(char::is_alphabetic)
        && heard
            .chars()
            .all(|c| is_cyrillic(c) || c == ' ' || c == '-');
    is_spoken.then(|| heard.to_lowercase())
}

fn is_cyrillic(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04FF}')
}

/// A term or variant as it may stand in a dictionary line: one line, without
/// the `|` and `,` separators, trimmed.
fn clean_field(text: &str, separators: &[char]) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() || separators.contains(&c) {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn render_line(term: &str, variants: &[String]) -> String {
    if variants.is_empty() {
        term.to_string()
    } else {
        format!("{term} | {}", variants.join(", "))
    }
}

/// Adds `term` with spoken `variants` to the dictionary text `content` and
/// returns the new text. A term already present (ignoring case) keeps its
/// line, takes the new spelling and gains the variants it lacked; otherwise a
/// line is appended. Comments and other lines are left as they are.
pub fn add_entry(content: &str, term: &str, variants: &[String]) -> (String, AddOutcome) {
    let term = clean_field(term, &['|', '#']);
    let term = term.trim_start_matches('#').trim().to_string();
    if term.is_empty() {
        return (content.to_string(), AddOutcome::Unchanged);
    }
    let mut new_variants: Vec<String> = Vec::new();
    for variant in variants {
        let variant = clean_field(variant, &['|', ',']).to_lowercase();
        if !variant.is_empty()
            && variant != term.to_lowercase()
            && !new_variants.contains(&variant)
        {
            new_variants.push(variant);
        }
    }

    let key = term.to_lowercase();
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    for line in lines.iter_mut() {
        let Some(entry) = DictEntry::parse(line) else {
            continue;
        };
        if entry.term.to_lowercase() != key {
            continue;
        }
        let mut merged = entry.variants.clone();
        for variant in &new_variants {
            if !merged.iter().any(|v| v.to_lowercase() == *variant) {
                merged.push(variant.clone());
            }
        }
        if entry.term == term && merged.len() == entry.variants.len() {
            return (content.to_string(), AddOutcome::Unchanged);
        }
        *line = render_line(&term, &merged);
        let mut out = lines.join("\n");
        if content.ends_with('\n') {
            out.push('\n');
        }
        return (out, AddOutcome::Updated);
    }

    let mut out = content.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&render_line(&term, &new_variants));
    out.push('\n');
    (out, AddOutcome::Added)
}

/// A word the user corrected right after a dictation was pasted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Correction {
    /// The spelling the user typed.
    pub term: String,
    /// What was pasted in its place, lowercased, when it is a Cyrillic spoken
    /// form worth keeping as a variant; empty otherwise.
    pub heard: String,
}

/// Longest field the correction check reads, in characters.
const MAX_FIELD_CHARS: usize = 20_000;
/// Most words on either side of a correction.
const MAX_CORRECTION_WORDS: usize = 3;
/// Longest corrected term, in characters.
const MAX_TERM_CHARS: usize = 40;

/// Characters that belong to a term: letters, digits and the joiners of
/// names like `C++`, `Node.js`, `CI/CD`, `.gitignore`.
fn is_term_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '+' | '#' | '.' | '_' | '-' | '/' | '\'' | '’' | '@')
}

fn trim_term(text: &str) -> &str {
    text.trim_matches(|c: char| {
        c.is_whitespace() || matches!(c, '.' | ',' | '!' | '?' | ':' | ';' | '«' | '»' | '"' | '(' | ')')
    })
}

/// The correction a user made inside freshly pasted text, if any.
///
/// `before` is the field's text right after `inserted` was pasted into it,
/// `after` the text later. The edit must replace one to three words inside
/// the pasted text with one to three others, and the new words must look like
/// a term: they contain a Latin letter or a digit, or a capital letter after
/// the first one (`GitHub`, `СберМаркет`). An ordinary Russian word fixed or
/// re-capitalized does not count, nor do pure insertions and deletions or
/// edits outside the pasted text.
pub fn detect_correction(before: &str, after: &str, inserted: &str) -> Option<Correction> {
    let old: Vec<char> = before.chars().collect();
    let new: Vec<char> = after.chars().collect();
    let pasted: Vec<char> = inserted.trim().chars().collect();
    if old == new
        || pasted.is_empty()
        || pasted.len() > old.len()
        || old.len() > MAX_FIELD_CHARS
        || new.len() > MAX_FIELD_CHARS
    {
        return None;
    }

    let mut prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let limit = old.len().min(new.len()) - prefix;
    let mut suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(limit)
        .take_while(|(a, b)| a == b)
        .count();
    while prefix > 0 && is_term_char(old[prefix - 1]) {
        prefix -= 1;
    }
    while suffix > 0 && is_term_char(old[old.len() - suffix]) {
        suffix -= 1;
    }
    let (old_start, old_end) = (prefix, old.len() - suffix);
    let new_end = new.len() - suffix;

    let inside_paste = (0..=old.len() - pasted.len()).any(|start| {
        old[start..start + pasted.len()] == pasted[..]
            && start <= old_start
            && old_end <= start + pasted.len()
    });
    if !inside_paste {
        return None;
    }

    let was: String = old[old_start..old_end].iter().collect();
    let now: String = new[prefix..new_end].iter().collect();
    let (was, now) = (trim_term(&was), trim_term(&now));
    let words = |text: &str| text.split_whitespace().count();
    if was.is_empty()
        || now.is_empty()
        || was == now
        || words(was) > MAX_CORRECTION_WORDS
        || words(now) > MAX_CORRECTION_WORDS
        || now.chars().count() < 2
        || now.chars().count() > MAX_TERM_CHARS
    {
        return None;
    }
    let looks_like_term = now.chars().any(|c| c.is_ascii_alphanumeric())
        || now.chars().skip(1).any(char::is_uppercase);
    if !looks_like_term {
        return None;
    }
    Some(Correction {
        term: now.to_string(),
        heard: spoken_variant(was).unwrap_or_default(),
    })
}

/// Formula starts a spreadsheet would run; such cells get a leading `'`.
fn is_formula_start(c: char) -> bool {
    matches!(c, '=' | '+' | '-' | '@' | '\t' | '\r')
}

fn csv_field(value: &str) -> String {
    let value = match value.chars().next() {
        Some(c) if is_formula_start(c) => format!("'{value}"),
        _ => value.to_string(),
    };
    format!("\"{}\"", value.replace('"', "\"\""))
}

/// The dictionary as CSV for a spreadsheet: a UTF-8 BOM, `;` between the two
/// columns «term» and «spoken» (variants joined with ", "), CRLF lines.
pub fn export_csv(content: &str) -> String {
    let mut out = String::from("\u{FEFF}term;spoken\r\n");
    for entry in parse_entries(content) {
        out.push_str(&csv_field(&entry.term));
        out.push(';');
        out.push_str(&csv_field(&entry.variants.join(", ")));
        out.push_str("\r\n");
    }
    out
}

/// Rows of a CSV text. The separator is whichever of `;`, `,` or tab occurs
/// most often outside quotes in the first line.
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let text = text.trim_start_matches('\u{FEFF}');
    let first_line = text.lines().next().unwrap_or("");
    let separator = [';', ',', '\t']
        .into_iter()
        .max_by_key(|sep| {
            let mut quoted = false;
            first_line
                .chars()
                .filter(|&c| {
                    if c == '"' {
                        quoted = !quoted;
                    }
                    !quoted && c == *sep
                })
                .count()
        })
        .unwrap_or(';');

    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' if quoted => quoted = false,
            '"' if field.is_empty() => quoted = true,
            c if c == separator && !quoted => row.push(std::mem::take(&mut field)),
            '\r' if !quoted => {}
            '\n' if !quoted => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            c => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// A cell without the `'` [`export_csv`] puts before a formula start.
fn unquote_cell(cell: &str) -> String {
    let cell = cell.trim();
    match cell.strip_prefix('\'') {
        Some(rest) if rest.chars().next().is_some_and(is_formula_start) => rest.to_string(),
        _ => cell.to_string(),
    }
}

/// Counts of an import.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportStats {
    pub added: usize,
    pub updated: usize,
    /// Rows without a term, header rows and rows already in the dictionary.
    pub skipped: usize,
}

/// Merges the CSV `csv` into the dictionary text `content`: the first column
/// is the term, the other columns hold spoken variants (several in one cell
/// are separated by commas). A first row whose term is a column name is a
/// header. Returns the new text.
pub fn import_csv(content: &str, csv: &str) -> (String, ImportStats) {
    const HEADERS: &[&str] = &["term", "word", "phrase", "правильно", "термин", "слово"];
    let mut text = content.to_string();
    let mut stats = ImportStats::default();
    for (index, row) in parse_csv(csv).into_iter().enumerate() {
        let term = row.first().map(|c| unquote_cell(c)).unwrap_or_default();
        if term.is_empty() || (index == 0 && HEADERS.contains(&term.to_lowercase().as_str())) {
            stats.skipped += 1;
            continue;
        }
        let variants: Vec<String> = row[1..]
            .iter()
            .flat_map(|cell| cell.split(',').map(unquote_cell).collect::<Vec<_>>())
            .filter(|v| !v.is_empty())
            .collect();
        let (next, outcome) = add_entry(&text, &term, &variants);
        text = next;
        match outcome {
            AddOutcome::Added => stats.added += 1,
            AddOutcome::Updated => stats.updated += 1,
            AddOutcome::Unchanged => stats.skipped += 1,
        }
    }
    (text, stats)
}

/// Text of the starter list as the app uses it: the copy in the config dir,
/// or the built-in one when that file is missing.
pub fn starter_text(dir: &Path) -> String {
    std::fs::read_to_string(dir.join(STARTER_FILE_NAME))
        .unwrap_or_else(|_| STARTER_DICTIONARY.to_string())
}

/// Counts of a dictionary text for the settings window.
#[repr(C)]
pub struct SPDictionaryStats {
    /// Distinct terms in the text.
    pub entries: u32,
    /// Of them, the ones the Omni instructions block keeps.
    pub kept: u32,
    /// Characters the kept ones take.
    pub chars: u32,
    pub max_entries: u32,
    pub max_chars: u32,
}

/// Counts for the dictionary text `content` (UTF-8, may be null).
///
/// # Safety
/// `content` must be null or a valid null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn sp_dictionary_stats(
    content: *const std::ffi::c_char,
) -> SPDictionaryStats {
    let text = unsafe { crate::ffi::cstr_to_str(content) }.unwrap_or("");
    let budget = user_budget(&parse_entries(text));
    SPDictionaryStats {
        entries: budget.entries as u32,
        kept: budget.kept as u32,
        chars: budget.chars as u32,
        max_entries: MAX_PROMPT_ENTRIES as u32,
        max_chars: MAX_PROMPT_CHARS as u32,
    }
}

/// The starter list text (see [`starter_text`]). Free with sp_core_free_string().
#[no_mangle]
pub extern "C" fn sp_dictionary_starter_text() -> *mut std::ffi::c_char {
    std::ffi::CString::new(starter_text(&crate::config::config_dir()))
        .unwrap_or_default()
        .into_raw()
}

fn c_text<'a>(ptr: *const std::ffi::c_char) -> &'a str {
    unsafe { crate::ffi::cstr_to_str(ptr) }.unwrap_or("")
}

fn into_c_string(text: String) -> *mut std::ffi::c_char {
    std::ffi::CString::new(text).unwrap_or_default().into_raw()
}

/// The user's dictionary file from the config (`dictionary.path`).
fn user_dictionary_path() -> std::path::PathBuf {
    match crate::config::load_config() {
        Ok(cfg) => crate::config::resolve_dictionary_path(&cfg),
        Err(_) => crate::config::config_dir().join("dictionary.txt"),
    }
}

/// Writes through a temporary file, so a reader never sees half a dictionary.
fn write_dictionary(path: &Path, content: &str) -> Result<()> {
    let temp = path.with_extension("txt.tmp");
    std::fs::write(&temp, content)
        .and_then(|_| std::fs::rename(&temp, path))
        .map_err(|e| CoreError::Config(format!("write {}: {e}", path.display())))
}

/// Adds `term` with comma-separated spoken `variants` (may be null) to the
/// user's dictionary file. Returns 1 when a line was added, 2 when an entry
/// was updated, 0 when nothing changed and -1 on an error.
///
/// # Safety
/// Both pointers must be null or valid null-terminated C strings.
#[no_mangle]
pub unsafe extern "C" fn sp_dictionary_add(
    term: *const std::ffi::c_char,
    variants: *const std::ffi::c_char,
) -> i32 {
    let variants: Vec<String> = c_text(variants).split(',').map(String::from).collect();
    let path = user_dictionary_path();
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let (next, outcome) = add_entry(&content, c_text(term), &variants);
    if outcome == AddOutcome::Unchanged {
        return 0;
    }
    if let Err(e) = write_dictionary(&path, &next) {
        log::error!("dictionary add: {e}");
        return -1;
    }
    log::info!("dictionary: {outcome:?} an entry");
    if outcome == AddOutcome::Added {
        1
    } else {
        2
    }
}

/// The correction found by [`detect_correction`] as JSON
/// `{"term": "...", "heard": "..."}`, or null when there is none or the user's
/// dictionary already has it. Free with sp_core_free_string().
///
/// # Safety
/// All pointers must be null or valid null-terminated C strings.
#[no_mangle]
pub unsafe extern "C" fn sp_dictionary_detect_correction(
    before: *const std::ffi::c_char,
    after: *const std::ffi::c_char,
    inserted: *const std::ffi::c_char,
) -> *mut std::ffi::c_char {
    let Some(correction) = detect_correction(c_text(before), c_text(after), c_text(inserted)) else {
        return std::ptr::null_mut();
    };
    let content = std::fs::read_to_string(user_dictionary_path()).unwrap_or_default();
    let variants = [correction.heard.clone()];
    if add_entry(&content, &correction.term, &variants).1 == AddOutcome::Unchanged {
        return std::ptr::null_mut();
    }
    into_c_string(
        serde_json::json!({ "term": correction.term, "heard": correction.heard }).to_string(),
    )
}

/// [`export_csv`] of the dictionary text `content`. Free with
/// sp_core_free_string().
///
/// # Safety
/// `content` must be null or a valid null-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn sp_dictionary_export_csv(
    content: *const std::ffi::c_char,
) -> *mut std::ffi::c_char {
    into_c_string(export_csv(c_text(content)))
}

/// [`import_csv`] of `csv` into the dictionary text `content`, as JSON
/// `{"content": "...", "added": n, "updated": n, "skipped": n}`. Free with
/// sp_core_free_string().
///
/// # Safety
/// Both pointers must be null or valid null-terminated C strings.
#[no_mangle]
pub unsafe extern "C" fn sp_dictionary_import_csv(
    content: *const std::ffi::c_char,
    csv: *const std::ffi::c_char,
) -> *mut std::ffi::c_char {
    let (text, stats) = import_csv(c_text(content), c_text(csv));
    into_c_string(
        serde_json::json!({
            "content": text,
            "added": stats.added,
            "updated": stats.updated,
            "skipped": stats.skipped,
        })
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# Заголовок файла
# Формат: ...

# Git и платформы разработки
GitHub | гитхаб, гит хаб, гитхуб
Git | гит

# Мессенджеры и соцсети
Telegram | телеграм, телега
WhatsApp | вотсап

# Омонимы с обычными русскими словами
Cursor | курсор
Go | го
";

    fn user(lines: &str) -> Vec<DictEntry> {
        parse_entries(lines)
    }

    #[test]
    fn parses_plain_and_variant_lines() {
        let entries = parse_entries("# c\nKubernetes | кубер, кубернетис\nPlain\n\nPlain\n |x\n");
        assert_eq!(
            entries,
            vec![
                DictEntry {
                    term: "Kubernetes".into(),
                    variants: vec!["кубер".into(), "кубернетис".into()]
                },
                DictEntry {
                    term: "Plain".into(),
                    variants: vec![]
                },
            ]
        );
    }

    #[test]
    fn sections_skip_the_file_header_and_keep_titles() {
        let sections = parse_sections(SAMPLE);
        let titles: Vec<&str> = sections.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Git и платформы разработки",
                "Мессенджеры и соцсети",
                "Омонимы с обычными русскими словами"
            ]
        );
        assert_eq!(sections[0].entries.len(), 2);
    }

    #[test]
    fn bundled_starter_list_parses_into_sections_with_homonyms_last() {
        let sections = parse_sections(STARTER_DICTIONARY);
        assert!(sections.len() >= 12);
        assert!(is_homonym_section(&sections.last().unwrap().title));
        assert!(sections.iter().all(|s| !s.entries.is_empty()));
        let total: usize = sections.iter().map(|s| s.entries.len()).sum();
        assert!(total > 400, "{total}");
    }

    #[test]
    fn user_entries_come_first_and_keep_their_variants() {
        let starter = parse_sections(SAMPLE);
        let block = build_prompt_block(
            &user("Grafana | графана\nПлотный"),
            &starter,
            AppCategory::Other,
            false,
        )
        .unwrap();
        let after = block.split("пиши так): ").nth(1).unwrap();
        assert!(after.starts_with("Grafana (графана); Плотный; "), "{block}");
    }

    #[test]
    fn matching_sections_precede_the_core() {
        let starter = parse_sections(SAMPLE);
        let block = build_prompt_block(&[], &starter, AppCategory::Messenger, false).unwrap();
        assert!(
            block.find("Telegram").unwrap() < block.find("GitHub").unwrap(),
            "{block}"
        );
        let dev = build_prompt_block(&[], &starter, AppCategory::Dev, false).unwrap();
        assert!(dev.find("GitHub").unwrap() < dev.find("Telegram").unwrap());
    }

    #[test]
    fn homonyms_only_for_developer_tools_and_as_bare_terms() {
        let starter = parse_sections(SAMPLE);
        let other = build_prompt_block(&[], &starter, AppCategory::Browser, false).unwrap();
        assert!(!other.contains("Cursor") && !other.contains("курсор"));
        let dev = build_prompt_block(&[], &starter, AppCategory::Dev, true).unwrap();
        assert!(dev.contains("Cursor, Go."), "{dev}");
        assert!(!dev.contains("курсор"));
    }

    #[test]
    fn user_entry_beats_a_starter_duplicate() {
        let starter = parse_sections(SAMPLE);
        let block = build_prompt_block(
            &user("github | гитхабчик"),
            &starter,
            AppCategory::Dev,
            false,
        )
        .unwrap();
        assert_eq!(block.matches("itHub").count() + block.matches("ithub").count(), 1);
    }

    #[test]
    fn the_real_starter_list_respects_the_caps_for_every_category() {
        let starter = parse_sections(STARTER_DICTIONARY);
        for category in [
            AppCategory::Messenger,
            AppCategory::Mail,
            AppCategory::Dev,
            AppCategory::Browser,
            AppCategory::Other,
        ] {
            let block = build_prompt_block(&[], &starter, category, true).unwrap();
            let entries = block.matches("; ").count() + 1;
            assert!(entries <= MAX_PROMPT_ENTRIES + 1, "{category:?}: {entries}");
            assert!(
                block.chars().count() < MAX_PROMPT_CHARS + 300,
                "{category:?}: {}",
                block.chars().count()
            );
        }
    }

    #[test]
    fn huge_user_dictionaries_are_capped_too() {
        let lines: String = (0..500).map(|i| format!("Term{i} | вариант{i}\n")).collect();
        let block = build_prompt_block(&user(&lines), &[], AppCategory::Other, false).unwrap();
        assert!(block.matches("Term").count() <= MAX_PROMPT_ENTRIES);
        assert!(block.chars().count() < MAX_PROMPT_CHARS + 300);
    }

    #[test]
    fn user_budget_counts_distinct_terms_and_what_fits() {
        let budget = user_budget(&user("Kubernetes | кубер\nkubernetes\nGrafana"));
        assert_eq!(budget.entries, 2);
        assert_eq!(budget.kept, 2);
        assert_eq!(budget.chars, "Kubernetes (кубер)".chars().count() + 2 + "Grafana".chars().count());

        let lines: String = (0..500).map(|i| format!("Term{i}\n")).collect();
        let budget = user_budget(&user(&lines));
        assert_eq!(budget.entries, 500);
        assert_eq!(budget.kept, MAX_PROMPT_ENTRIES);

        let long: String = (0..40).map(|i| format!("Term{i} | {}\n", "вариант".repeat(10))).collect();
        let budget = user_budget(&user(&long));
        assert!(budget.kept < 40 && budget.chars <= MAX_PROMPT_CHARS, "{budget:?}");
    }

    #[test]
    fn user_budget_matches_the_block() {
        let lines: String = (0..200).map(|i| format!("Term{i} | вариант{i}\n")).collect();
        let entries = user(&lines);
        let block = build_prompt_block(&entries, &[], AppCategory::Other, false).unwrap();
        assert_eq!(block.matches("Term").count(), user_budget(&entries).kept);
    }

    #[test]
    fn stats_over_ffi() {
        let text = std::ffi::CString::new("A\nB | б\n# c\n").unwrap();
        let stats = unsafe { sp_dictionary_stats(text.as_ptr()) };
        assert_eq!((stats.entries, stats.kept, stats.max_entries), (2, 2, 120));
        let empty = unsafe { sp_dictionary_stats(std::ptr::null()) };
        assert_eq!(empty.entries, 0);
    }

    #[test]
    fn starter_text_falls_back_to_the_built_in_list() {
        let dir = std::env::temp_dir().join("vm-dictionary-starter-missing");
        assert_eq!(starter_text(&dir), STARTER_DICTIONARY);
    }

    #[test]
    fn add_entry_appends_a_new_line() {
        let (text, outcome) = add_entry("# мои\nGrafana", "Kubernetes", &["Кубер".into(), "".into()]);
        assert_eq!(outcome, AddOutcome::Added);
        assert_eq!(text, "# мои\nGrafana\nKubernetes | кубер\n");
        let (text, _) = add_entry("", "Grafana", &[]);
        assert_eq!(text, "Grafana\n");
    }

    #[test]
    fn add_entry_merges_variants_and_takes_the_new_spelling() {
        let content = "kubernetes | кубер\nGit\n";
        let (text, outcome) = add_entry(content, "Kubernetes", &["кубернетис".into(), "Кубер".into()]);
        assert_eq!(outcome, AddOutcome::Updated);
        assert_eq!(text, "Kubernetes | кубер, кубернетис\nGit\n");
        let (same, outcome) = add_entry(&text, "Kubernetes", &["кубер".into()]);
        assert_eq!(outcome, AddOutcome::Unchanged);
        assert_eq!(same, text);
    }

    #[test]
    fn add_entry_keeps_separators_out_of_fields() {
        let (text, _) = add_entry("", " # A|B \n", &["раз, два".into(), "три|четыре".into()]);
        assert_eq!(text, "A B | раз два, три четыре\n");
        assert_eq!(add_entry("x\n", "  ", &[]).1, AddOutcome::Unchanged);
    }

    fn correction(term: &str, heard: &str) -> Option<Correction> {
        Some(Correction {
            term: term.into(),
            heard: heard.into(),
        })
    }

    #[test]
    fn a_respelled_word_inside_the_paste_is_a_correction() {
        let inserted = "Разверни это в кубер, пожалуйста.";
        let before = format!("Привет. {inserted}");
        let after = "Привет. Разверни это в Kubernetes, пожалуйста.";
        assert_eq!(detect_correction(&before, after, inserted), correction("Kubernetes", "кубер"));
    }

    #[test]
    fn corrections_of_latin_words_and_case_count() {
        let inserted = "Залей на гитхаб и в github actions";
        assert_eq!(
            detect_correction(inserted, "Залей на GitHub и в github actions", inserted),
            correction("GitHub", "гитхаб")
        );
        assert_eq!(
            detect_correction(inserted, "Залей на гитхаб и в GitHub Actions", inserted),
            correction("GitHub Actions", "")
        );
        assert_eq!(
            detect_correction("Открой Kubernetis.", "Открой Kubernetes.", "Открой Kubernetis."),
            correction("Kubernetes", "")
        );
        assert_eq!(
            detect_correction("купи в сбер маркете", "купи в СберМаркете", "купи в сбер маркете"),
            correction("СберМаркете", "сбер маркете")
        );
    }

    #[test]
    fn terms_with_joiners_are_whole() {
        let inserted = "пишу на си плюс плюс и ноде";
        assert_eq!(
            detect_correction(inserted, "пишу на C++ и ноде", inserted),
            correction("C++", "си плюс плюс")
        );
        assert_eq!(
            detect_correction(inserted, "пишу на си плюс плюс и Node.js", inserted),
            correction("Node.js", "ноде")
        );
    }

    #[test]
    fn ordinary_russian_fixes_are_not_terms() {
        let inserted = "привет как дела";
        assert_eq!(detect_correction(inserted, "Привет как дела", inserted), None);
        assert_eq!(detect_correction(inserted, "привет как делишки", inserted), None);
        assert_eq!(detect_correction(inserted, "привет, как дела", inserted), None);
    }

    #[test]
    fn insertions_deletions_and_edits_elsewhere_are_not_corrections() {
        let inserted = "запусти докер";
        let before = format!("Старый текст. {inserted}");
        assert_eq!(detect_correction(&before, "Старый текст. запусти", inserted), None);
        assert_eq!(
            detect_correction(&before, "Старый текст. запусти докер и Docker Compose", inserted),
            None
        );
        assert_eq!(detect_correction(&before, "Новый Text. запусти докер", inserted), None);
        assert_eq!(detect_correction(&before, &before, inserted), None);
        assert_eq!(detect_correction(&before, "Старый текст. Docker", ""), None);
        assert_eq!(detect_correction(&before, "Старый текст. запусти D", inserted), None);
    }

    #[test]
    fn rewrites_of_many_words_are_not_corrections() {
        let inserted = "один два три четыре пять";
        assert_eq!(
            detect_correction(inserted, "One Two Three Four пять", inserted),
            None
        );
    }

    #[test]
    fn the_paste_may_sit_anywhere_in_the_field() {
        let inserted = "через кубер";
        let before = "через кубер — это начало. Потом через кубер ещё раз.";
        let after = "через кубер — это начало. Потом через Kubernetes ещё раз.";
        assert_eq!(detect_correction(before, after, inserted), correction("Kubernetes", "кубер"));
    }

    #[test]
    fn csv_export_quotes_and_defuses_formulas() {
        let csv = export_csv("# c\nKubernetes | кубер, кубернетис\n+7 Store\nSay \"hi\"\n");
        assert_eq!(
            csv,
            "\u{FEFF}term;spoken\r\n\"Kubernetes\";\"кубер, кубернетис\"\r\n\"'+7 Store\";\"\"\r\n\"Say \"\"hi\"\"\";\"\"\r\n"
        );
    }

    #[test]
    fn csv_round_trip_restores_the_entries() {
        let content = "Kubernetes | кубер, кубернетис\n+7 Store\nSay \"hi\"\n";
        let (text, stats) = import_csv("", &export_csv(content));
        assert_eq!(parse_entries(&text), parse_entries(content));
        assert_eq!(stats, ImportStats { added: 3, updated: 0, skipped: 1 });
    }

    #[test]
    fn csv_import_takes_commas_tabs_and_extra_columns() {
        let (text, stats) = import_csv(
            "Git\nkubernetes | кубер\n",
            "Kubernetes,\"кубернетис, кубик\"\nGrafana,графана,графа\n,пусто\nGit,\n",
        );
        assert_eq!(text, "Git\nKubernetes | кубер, кубернетис, кубик\nGrafana | графана, графа\n");
        assert_eq!(stats, ImportStats { added: 1, updated: 1, skipped: 2 });

        let (text, _) = import_csv("", "Слово\tкак произносят\r\nJira\tджира\r\n");
        assert_eq!(text, "Jira | джира\n");
    }

    #[test]
    fn detect_over_ffi_skips_what_the_dictionary_has() {
        let before = std::ffi::CString::new("в кубер").unwrap();
        let after = std::ffi::CString::new("в Kubernetes").unwrap();
        let none = std::ffi::CString::new("в кубер нет").unwrap();
        let found =
            unsafe { sp_dictionary_detect_correction(before.as_ptr(), after.as_ptr(), before.as_ptr()) };
        if !found.is_null() {
            let json = unsafe { std::ffi::CStr::from_ptr(found) }.to_str().unwrap().to_string();
            unsafe { crate::sp_core_free_string(found) };
            assert!(json.contains("\"term\":\"Kubernetes\""), "{json}");
        }
        let missing =
            unsafe { sp_dictionary_detect_correction(before.as_ptr(), after.as_ptr(), none.as_ptr()) };
        assert!(missing.is_null());
    }

    #[test]
    fn empty_input_gives_no_block() {
        assert_eq!(build_prompt_block(&[], &[], AppCategory::Dev, true), None);
    }

    #[test]
    fn specific_sections_leave_room_for_the_core() {
        let starter = parse_sections(STARTER_DICTIONARY);
        let block = build_prompt_block(&[], &starter, AppCategory::Dev, false).unwrap();
        assert!(block.contains("Telegram") || block.contains("Visa") || block.contains("ChatGPT"), "{block}");
    }
}
