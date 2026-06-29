use std::collections::HashMap;
use crate::style::{ContainerStyle, AnimationIterationCount};

#[derive(Clone, Debug)]
pub struct Keyframe {
    pub percentage: f32, // 0.0 to 1.0
    pub declarations: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct KeyframesAnimation {
    pub name: String,
    pub keyframes: Vec<Keyframe>,
}

pub fn parse_duration_sec(val: &str) -> f32 {
    let val = val.trim().to_lowercase();
    if let Some(stripped) = val.strip_suffix("ms") {
        stripped.parse::<f32>().unwrap_or(0.0) / 1000.0
    } else if let Some(stripped) = val.strip_suffix("s") {
        stripped.parse::<f32>().unwrap_or(0.0)
    } else {
        val.parse::<f32>().unwrap_or(0.0)
    }
}

pub fn parse_animation_shorthand(val: &str, current_style: &mut ContainerStyle) {
    let parts: Vec<&str> = val.split_whitespace().collect();
    let mut durations_found = 0;
    
    for part in parts {
        let part_lower = part.to_lowercase();
        if part_lower.ends_with("s") || part_lower.ends_with("ms") {
            let duration = parse_duration_sec(part);
            if durations_found == 0 {
                current_style.animation_duration = duration;
            } else if durations_found == 1 {
                current_style.animation_delay = duration;
            }
            durations_found += 1;
            continue;
        }
        
        if part_lower == "infinite" {
            current_style.animation_iteration_count = AnimationIterationCount::Infinite;
            continue;
        }
        
        if ["linear", "ease", "ease-in", "ease-out", "ease-in-out"].contains(&part_lower.as_str()) || part_lower.starts_with("cubic-bezier(") {
            current_style.animation_timing_function = std::sync::Arc::from(part);
            continue;
        }
        
        if ["normal", "reverse", "alternate", "alternate-reverse"].contains(&part_lower.as_str()) {
            current_style.animation_direction = std::sync::Arc::from(part_lower);
            continue;
        }
        
        if ["none", "forwards", "backwards", "both"].contains(&part_lower.as_str()) {
            current_style.animation_fill_mode = std::sync::Arc::from(part_lower);
            continue;
        }
        
        if ["running", "paused"].contains(&part_lower.as_str()) {
            current_style.animation_play_state = std::sync::Arc::from(part_lower);
            continue;
        }
        
        if let Ok(num) = part_lower.parse::<f32>() {
            current_style.animation_iteration_count = AnimationIterationCount::Count(num);
            continue;
        }
        
        current_style.animation_name = Some(std::sync::Arc::from(part));
    }
}

pub fn strip_css_comments(css: &str) -> String {
    let mut result = String::new();
    let mut chars = css.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(c2) = chars.next() {
                if c2 == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

pub fn parse_keyframes(css: &str) -> HashMap<String, KeyframesAnimation> {
    let css = strip_css_comments(css);
    let mut animations = HashMap::new();
    
    let mut pos = 0;
    while let Some(start_idx) = css[pos..].find("@keyframes") {
        let abs_start = pos + start_idx;
        let rest = &css[abs_start + 10..];
        
        if let Some(brace_idx) = rest.find('{') {
            let name = rest[..brace_idx].trim().to_string();
            let block_content_start = abs_start + 10 + brace_idx + 1;
            let mut brace_count = 1;
            let mut end_idx = None;
            for (idx, c) in css[block_content_start..].char_indices() {
                if c == '{' {
                    brace_count += 1;
                } else if c == '}' {
                    brace_count -= 1;
                    if brace_count == 0 {
                        end_idx = Some(block_content_start + idx);
                        break;
                    }
                }
            }
            
            if let Some(abs_end) = end_idx {
                let block_content = &css[block_content_start..abs_end];
                let keyframes = parse_keyframe_blocks(block_content);
                animations.insert(name.clone(), KeyframesAnimation {
                    name,
                    keyframes,
                });
                pos = abs_end + 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    animations
}

fn parse_keyframe_blocks(content: &str) -> Vec<Keyframe> {
    let mut keyframes = Vec::new();
    let mut pos = 0;
    while pos < content.len() {
        let rest = &content[pos..];
        if let Some(brace_idx) = rest.find('{') {
            let selector = rest[..brace_idx].trim();
            let decl_start = pos + brace_idx + 1;
            if let Some(end_brace_idx) = content[decl_start..].find('}') {
                let decl_content = &content[decl_start..decl_start + end_brace_idx];
                let declarations = parse_declarations(decl_content);
                
                for sel in selector.split(',') {
                    let sel = sel.trim();
                    let percentage = if sel.eq_ignore_ascii_case("from") {
                        0.0
                    } else if sel.eq_ignore_ascii_case("to") {
                        1.0
                    } else if sel.ends_with('%') {
                        sel.trim_end_matches('%').parse::<f32>().unwrap_or(0.0) / 100.0
                    } else {
                        continue;
                    };
                    keyframes.push(Keyframe {
                        percentage,
                        declarations: declarations.clone(),
                    });
                }
                pos = decl_start + end_brace_idx + 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    keyframes.sort_by(|a, b| a.percentage.partial_cmp(&b.percentage).unwrap_or(std::cmp::Ordering::Equal));
    keyframes
}

fn parse_declarations(content: &str) -> Vec<(String, String)> {
    let mut decls = Vec::new();
    let tokenizer = simplecss::DeclarationTokenizer::from(content);
    for decl in tokenizer {
        decls.push((decl.name.to_lowercase(), decl.value.to_string()));
    }
    decls
}
