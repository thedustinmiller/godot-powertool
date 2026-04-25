use std::{
    fs,
    path::Path,
};

use anyhow::{Context, Result};
use quick_xml::{Reader, events::Event};

use crate::{bbcode, class_list};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DescriptionMode {
    #[default]
    None,
    FirstSentence,
    Brief,
    Full,
}

#[derive(Debug, Clone)]
pub struct ConversionConfig {
    pub class_description: DescriptionMode,
    pub method_descriptions: DescriptionMode,
    pub property_descriptions: DescriptionMode,
    pub signal_descriptions: DescriptionMode,
    pub constant_descriptions: DescriptionMode,
    pub max_enum_values: usize,
    pub no_virtual: bool,
    pub compact_format: bool,
    pub simple_signals: bool,
}

impl Default for ConversionConfig {
    fn default() -> Self {
        Self {
            class_description: DescriptionMode::FirstSentence,
            method_descriptions: DescriptionMode::None,
            property_descriptions: DescriptionMode::None,
            signal_descriptions: DescriptionMode::None,
            constant_descriptions: DescriptionMode::None,
            max_enum_values: 10,
            no_virtual: true,
            compact_format: true,
            simple_signals: true,
        }
    }
}

fn should_skip_class(name: &str) -> bool {
    let skip_exact = ["@GlobalScope", "@GDScript"];
    if skip_exact.contains(&name) {
        return true;
    }
    if name.starts_with("Editor") || name.starts_with('_') {
        return true;
    }
    if name != "AudioServer" && (name.ends_with("Plugin") || name.ends_with("Server")) {
        return true;
    }
    false
}

fn get_description(text: &str, mode: DescriptionMode) -> String {
    if mode == DescriptionMode::None || text.is_empty() {
        return String::new();
    }
    match mode {
        DescriptionMode::FirstSentence => bbcode::first_sentence(text),
        _ => bbcode::convert_bbcode(text),
    }
}

// Simple XML-to-struct parsing for Godot class docs.
// We parse the full file into a structured representation, then format to markdown.

struct ClassDoc {
    name: String,
    inherits: String,
    brief_description: String,
    description: String,
    members: Vec<Member>,
    methods: Vec<Method>,
    signals: Vec<Signal>,
    constants: Vec<Constant>,
}

struct Member {
    name: String,
    type_name: String,
    default: String,
    enum_name: Option<String>,
    description: String,
}

struct Method {
    name: String,
    qualifiers: String,
    return_type: String,
    params: Vec<Param>,
    description: String,
}

struct Signal {
    name: String,
    params: Vec<Param>,
    description: String,
}

struct Param {
    name: String,
    type_name: String,
    default: Option<String>,
}

struct Constant {
    name: String,
    value: String,
    enum_name: String,
    description: String,
}

fn parse_xml_class(xml_path: &Path) -> Result<Option<ClassDoc>> {
    let content = fs::read_to_string(xml_path)
        .with_context(|| format!("Failed to read {}", xml_path.display()))?;

    let mut reader = Reader::from_str(&content);

    let mut doc = ClassDoc {
        name: String::new(),
        inherits: String::new(),
        brief_description: String::new(),
        description: String::new(),
        members: Vec::new(),
        methods: Vec::new(),
        signals: Vec::new(),
        constants: Vec::new(),
    };

    let mut buf = Vec::new();
    let mut current_section = String::new();
    let mut text_target: Option<&mut String> = Option::None;
    let mut current_method: Option<Method> = Option::None;
    let mut current_signal: Option<Signal> = Option::None;
    let mut in_method_desc = false;
    let mut in_signal_desc = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();

                match tag.as_str() {
                    "class" => {
                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let val =
                                String::from_utf8_lossy(&attr.value).to_string();
                            match key.as_str() {
                                "name" => doc.name = val,
                                "inherits" => doc.inherits = val,
                                _ => {}
                            }
                        }
                    }
                    "brief_description" => {
                        text_target = Some(&mut doc.brief_description);
                    }
                    "description" if current_section.is_empty() => {
                        text_target = Some(&mut doc.description);
                    }
                    "description" if current_method.is_some() => {
                        in_method_desc = true;
                    }
                    "description" if current_signal.is_some() => {
                        in_signal_desc = true;
                    }
                    "members" => current_section = "members".to_string(),
                    "methods" => current_section = "methods".to_string(),
                    "signals" => current_section = "signals".to_string(),
                    "constants" => current_section = "constants".to_string(),
                    "member" if current_section == "members" => {
                        let mut member = Member {
                            name: String::new(),
                            type_name: String::new(),
                            default: String::new(),
                            enum_name: Option::None,
                            description: String::new(),
                        };
                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let val =
                                String::from_utf8_lossy(&attr.value).to_string();
                            match key.as_str() {
                                "name" => member.name = val,
                                "type" => member.type_name = val,
                                "default" => member.default = val,
                                "enum" => member.enum_name = Some(val),
                                _ => {}
                            }
                        }
                        doc.members.push(member);
                    }
                    "method" if current_section == "methods" => {
                        let mut method = Method {
                            name: String::new(),
                            qualifiers: String::new(),
                            return_type: String::new(),
                            params: Vec::new(),
                            description: String::new(),
                        };
                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let val =
                                String::from_utf8_lossy(&attr.value).to_string();
                            match key.as_str() {
                                "name" => method.name = val,
                                "qualifiers" => method.qualifiers = val,
                                _ => {}
                            }
                        }
                        current_method = Some(method);
                    }
                    "return" if current_method.is_some() => {
                        if let Some(ref mut m) = current_method {
                            for attr in e.attributes().filter_map(|a| a.ok()) {
                                let key =
                                    String::from_utf8_lossy(attr.key.as_ref()).to_string();
                                let val =
                                    String::from_utf8_lossy(&attr.value).to_string();
                                if key == "type" {
                                    m.return_type = val;
                                }
                            }
                        }
                    }
                    "param" if current_method.is_some() => {
                        let mut param = Param {
                            name: String::new(),
                            type_name: String::new(),
                            default: Option::None,
                        };
                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let val =
                                String::from_utf8_lossy(&attr.value).to_string();
                            match key.as_str() {
                                "name" => param.name = val,
                                "type" => param.type_name = val,
                                "default" => param.default = Some(val),
                                _ => {}
                            }
                        }
                        if let Some(ref mut m) = current_method {
                            m.params.push(param);
                        }
                    }
                    "signal" if current_section == "signals" => {
                        let mut signal = Signal {
                            name: String::new(),
                            params: Vec::new(),
                            description: String::new(),
                        };
                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let val =
                                String::from_utf8_lossy(&attr.value).to_string();
                            if key == "name" {
                                signal.name = val;
                            }
                        }
                        current_signal = Some(signal);
                    }
                    "param" if current_signal.is_some() => {
                        let mut param = Param {
                            name: String::new(),
                            type_name: String::new(),
                            default: Option::None,
                        };
                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let val =
                                String::from_utf8_lossy(&attr.value).to_string();
                            match key.as_str() {
                                "name" => param.name = val,
                                "type" => param.type_name = val,
                                _ => {}
                            }
                        }
                        if let Some(ref mut s) = current_signal {
                            s.params.push(param);
                        }
                    }
                    "constant" if current_section == "constants" => {
                        let mut constant = Constant {
                            name: String::new(),
                            value: String::new(),
                            enum_name: "Constants".to_string(),
                            description: String::new(),
                        };
                        for attr in e.attributes().filter_map(|a| a.ok()) {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let val =
                                String::from_utf8_lossy(&attr.value).to_string();
                            match key.as_str() {
                                "name" => constant.name = val,
                                "value" => constant.value = val,
                                "enum" => constant.enum_name = val,
                                _ => {}
                            }
                        }
                        doc.constants.push(constant);
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match tag.as_str() {
                    "brief_description" | "description"
                        if text_target.is_some() && current_section.is_empty() =>
                    {
                        text_target = Option::None;
                    }
                    "description" if in_method_desc => {
                        in_method_desc = false;
                    }
                    "description" if in_signal_desc => {
                        in_signal_desc = false;
                    }
                    "members" | "methods" | "signals" | "constants" => {
                        current_section.clear();
                    }
                    "method" if current_method.is_some() => {
                        doc.methods.push(current_method.take().unwrap());
                    }
                    "signal" if current_signal.is_some() => {
                        doc.signals.push(current_signal.take().unwrap());
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string();
                if let Some(ref mut target) = text_target {
                    target.push_str(&text);
                } else if in_method_desc {
                    if let Some(ref mut m) = current_method {
                        m.description.push_str(&text);
                    }
                } else if in_signal_desc {
                    if let Some(ref mut s) = current_signal {
                        s.description.push_str(&text);
                    }
                } else if current_section == "constants" {
                    // Text content of <constant> element is its description
                    if let Some(last) = doc.constants.last_mut() {
                        last.description.push_str(&text);
                    }
                } else if current_section == "members" {
                    // Text content of <member> element is its description
                    if let Some(last) = doc.members.last_mut() {
                        last.description.push_str(&text);
                    }
                }
            }
            Err(e) => {
                eprintln!("XML parse error in {}: {e}", xml_path.display());
                return Ok(Option::None);
            }
            _ => {}
        }
        buf.clear();
    }

    if doc.name.is_empty() || should_skip_class(&doc.name) {
        return Ok(Option::None);
    }

    Ok(Some(doc))
}

fn format_class(doc: &ClassDoc, config: &ConversionConfig) -> String {
    let mut lines = Vec::new();

    // Header
    if config.compact_format && !doc.inherits.is_empty() {
        lines.push(format!("## {} <- {}", doc.name, doc.inherits));
    } else {
        lines.push(format!("## {}", doc.name));
        if !doc.inherits.is_empty() {
            lines.push(format!("Inherits: {}", doc.inherits));
        }
    }
    lines.push(String::new());

    // Class description
    let class_desc = match config.class_description {
        DescriptionMode::None => String::new(),
        DescriptionMode::FirstSentence => bbcode::first_sentence(&doc.brief_description),
        DescriptionMode::Brief => bbcode::convert_bbcode(&doc.brief_description),
        DescriptionMode::Full => {
            if !doc.description.is_empty() {
                bbcode::convert_bbcode(&doc.description)
            } else {
                bbcode::convert_bbcode(&doc.brief_description)
            }
        }
    };
    if !class_desc.is_empty() {
        lines.push(class_desc);
        lines.push(String::new());
    }

    // Properties
    if !doc.members.is_empty() {
        lines.push("**Props:**".to_string());
        for m in &doc.members {
            let type_str = if let Some(ref enum_name) = m.enum_name {
                format!("{} ({})", m.type_name, enum_name)
            } else {
                m.type_name.clone()
            };
            if m.default.is_empty() {
                lines.push(format!("- {}: {}", m.name, type_str));
            } else {
                lines.push(format!("- {}: {} = {}", m.name, type_str, m.default));
            }
        }

        if config.property_descriptions != DescriptionMode::None {
            lines.push(String::new());
            for m in &doc.members {
                let desc = get_description(&m.description, config.property_descriptions);
                if !desc.is_empty() {
                    lines.push(format!("- **{}**: {}", m.name, desc));
                }
            }
        }
        lines.push(String::new());
    }

    // Methods
    let method_lines: Vec<String> = doc
        .methods
        .iter()
        .filter(|m| {
            if config.no_virtual && m.qualifiers.contains("virtual") {
                return false;
            }
            true
        })
        .map(|m| {
            let params: Vec<String> = m
                .params
                .iter()
                .map(|p| {
                    if let Some(ref def) = p.default {
                        format!("{}: {} = {}", p.name, p.type_name, def)
                    } else {
                        format!("{}: {}", p.name, p.type_name)
                    }
                })
                .collect();
            let ret = if m.return_type.is_empty() || m.return_type == "void" {
                String::new()
            } else {
                format!(" -> {}", m.return_type)
            };
            let desc = get_description(&m.description, config.method_descriptions);
            let desc_str = if desc.is_empty() {
                String::new()
            } else {
                format!(" - {desc}")
            };
            format!("- {}({}){ret}{desc_str}", m.name, params.join(", "))
        })
        .collect();

    if !method_lines.is_empty() {
        lines.push("**Methods:**".to_string());
        lines.extend(method_lines);
        lines.push(String::new());
    }

    // Signals
    if !doc.signals.is_empty() {
        lines.push("**Signals:**".to_string());
        for s in &doc.signals {
            let params: Vec<String> = s
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name, p.type_name))
                .collect();
            let param_str = if config.simple_signals && params.is_empty() {
                String::new()
            } else {
                format!("({})", params.join(", "))
            };
            let desc = get_description(&s.description, config.signal_descriptions);
            let desc_str = if desc.is_empty() {
                String::new()
            } else {
                format!(" - {desc}")
            };
            lines.push(format!("- {}{param_str}{desc_str}", s.name));
        }
        lines.push(String::new());
    }

    // Enums/Constants
    if !doc.constants.is_empty() {
        // Group by enum name
        let mut enums: Vec<(String, Vec<&Constant>)> = Vec::new();
        for c in &doc.constants {
            if let Some(group) = enums.iter_mut().find(|(name, _)| name == &c.enum_name) {
                group.1.push(c);
            } else {
                enums.push((c.enum_name.clone(), vec![c]));
            }
        }

        lines.push("**Enums:**".to_string());
        for (enum_name, values) in &enums {
            let value_strs: Vec<String> = values
                .iter()
                .take(config.max_enum_values)
                .map(|c| format!("{}={}", c.name, c.value))
                .collect();
            let mut display = value_strs.join(", ");
            if values.len() > config.max_enum_values {
                display.push_str(", ...");
            }
            lines.push(format!("**{enum_name}:** {display}"));

            if config.constant_descriptions != DescriptionMode::None {
                for c in values {
                    let desc = get_description(&c.description, config.constant_descriptions);
                    if !desc.is_empty() {
                        lines.push(format!("  - {}: {desc}", c.name));
                    }
                }
            }
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

/// Convert all XML files in a directory to per-class markdown + index files.
pub fn convert_directory_split(
    input_dir: &Path,
    output_dir: &Path,
    config: &ConversionConfig,
    classes_filter: Option<&[String]>,
) -> Result<()> {
    fs::create_dir_all(output_dir)?;

    let mut xml_files: Vec<_> = fs::read_dir(input_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext == "xml")
        })
        .map(|e| e.path())
        .collect();
    xml_files.sort();

    // Apply class filter
    let filter_set: Option<std::collections::HashSet<&str>> = classes_filter.map(|classes| {
        classes.iter().map(|s| s.as_str()).collect()
    });
    if let Some(ref filter) = filter_set {
        xml_files.retain(|f| {
            f.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|name| filter.contains(name))
        });
    }

    let unified_set: std::collections::HashSet<&str> =
        class_list::CLASS_UNIFIED.iter().copied().collect();

    let mut common_entries: Vec<(String, String, String)> = Vec::new();
    let mut other_entries: Vec<(String, String, String)> = Vec::new();
    let mut converted = 0u32;
    let mut skipped = 0u32;

    // Per-class files use FULL class descriptions for maximum detail
    let detail_config = ConversionConfig {
        class_description: DescriptionMode::Full,
        method_descriptions: config.method_descriptions,
        property_descriptions: config.property_descriptions,
        signal_descriptions: config.signal_descriptions,
        constant_descriptions: config.constant_descriptions,
        max_enum_values: config.max_enum_values,
        no_virtual: config.no_virtual,
        compact_format: config.compact_format,
        simple_signals: config.simple_signals,
    };

    for xml_file in &xml_files {
        let doc = match parse_xml_class(xml_file)? {
            Some(doc) => doc,
            Option::None => {
                skipped += 1;
                continue;
            }
        };

        let brief = bbcode::first_sentence(&doc.brief_description);
        let name = doc.name.clone();
        let inherits = doc.inherits.clone();

        // Write per-class file
        let md = format_class(&doc, &detail_config);
        fs::write(output_dir.join(format!("{name}.md")), format!("{md}\n"))?;
        converted += 1;

        if unified_set.contains(name.as_str()) {
            common_entries.push((name, inherits, brief));
        } else {
            other_entries.push((name, inherits, brief));
        }
    }

    // Write index files
    write_index(
        &output_dir.join("_common.md"),
        &format!("Common Classes ({})", common_entries.len()),
        &mut common_entries,
    )?;
    write_index(
        &output_dir.join("_other.md"),
        &format!("Other Classes ({})", other_entries.len()),
        &mut other_entries,
    )?;

    println!("Converted {converted} classes, skipped {skipped}");
    println!(
        "Common index: {} ({} classes)",
        output_dir.join("_common.md").display(),
        common_entries.len()
    );
    println!(
        "Other index: {} ({} classes)",
        output_dir.join("_other.md").display(),
        other_entries.len()
    );

    Ok(())
}

fn write_index(
    path: &Path,
    title: &str,
    entries: &mut [(String, String, String)],
) -> Result<()> {
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut lines = vec![format!("# {title}"), String::new()];
    for (name, inherits, brief) in entries.iter() {
        let parent = if inherits.is_empty() {
            String::new()
        } else {
            format!(" <- {inherits}")
        };
        let desc = if brief.is_empty() {
            String::new()
        } else {
            format!(" — {brief}")
        };
        lines.push(format!("- {name}{parent}{desc}"));
    }
    lines.push(String::new());
    fs::write(path, lines.join("\n"))?;
    Ok(())
}
