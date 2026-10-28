//! Turns the app's cached danmaku.xml into an ASS subtitle shown beside the MP4.
use regex::{Captures, Regex};

const WIDTH: f64 = 1920.0;
const HEIGHT: f64 = 1080.0;
/// The app's default size 25 is drawn at 50 px on a 1080p canvas.
const SCALE: f64 = 2.0;
// shortcut: every lane is sized for the default font, so rare larger comments may touch their neighbours; size lanes per comment if users report overlap.
const LANE: f64 = 25.0 * SCALE + 4.0;
const SCROLL_SECONDS: f64 = 8.0;
const FIXED_SECONDS: f64 = 4.0;
const GAP: f64 = 20.0;

enum Kind {
    Scroll,
    Top,
    Bottom,
}

/// How many comments `to_ass` would draw; shown on the list row.
pub fn count(xml: &str) -> usize {
    comments(xml).len()
}

/// Returns None when the file holds no comment this converter can draw (e.g. danmaku disabled for the video).
pub fn to_ass(xml: &str) -> Option<String> {
    let mut comments = comments(xml);
    if comments.is_empty() {
        return None;
    }
    comments.sort_by(|a, b| a.0.total_cmp(&b.0));

    let lanes = (HEIGHT / LANE) as usize;
    let mut scroll: Vec<Option<(f64, f64)>> = vec![None; lanes];
    let (mut top, mut bottom) = (vec![f64::MIN; lanes], vec![f64::MIN; lanes]);
    let mut out = String::from(HEADER);
    for (time, kind, size, color, text) in comments {
        let width = text.chars().map(|c| if c.is_ascii() { 0.5 } else { 1.0 }).sum::<f64>() * size;
        let (end, placement) = match kind {
            Kind::Scroll => {
                let lane = scroll_lane(&scroll, time, width);
                scroll[lane] = Some((time, width));
                let y = lane as f64 * LANE;
                (time + SCROLL_SECONDS, format!(r"\an7\move({WIDTH},{y},{},{y})", -width))
            }
            Kind::Top | Kind::Bottom => {
                let ends = if matches!(kind, Kind::Top) { &mut top } else { &mut bottom };
                let lane = fixed_lane(ends, time);
                ends[lane] = time + FIXED_SECONDS;
                let y = lane as f64 * LANE;
                let placement =
                    if matches!(kind, Kind::Top) { format!(r"\an8\pos({},{y})", WIDTH / 2.0) } else { format!(r"\an2\pos({},{})", WIDTH / 2.0, HEIGHT - y) };
                (time + FIXED_SECONDS, placement)
            }
        };
        let mut tags = placement;
        if size != 25.0 * SCALE {
            tags += &format!(r"\fs{size}");
        }
        if color != 0xFFFFFF {
            tags += &format!(r"\c&H{:02X}{:02X}{:02X}&", color & 0xFF, (color >> 8) & 0xFF, color >> 16);
        }
        out += &format!("Dialogue: 0,{},{},Danmaku,,0,0,0,,{{{tags}}}{text}\n", clock(time), clock(end));
    }
    Some(out)
}

/// (time, kind, font size, RGB color, text) of every drawable comment, in file order.
fn comments(xml: &str) -> Vec<(f64, Kind, f64, u32, String)> {
    Regex::new(r#"<d p="([^"]*)">([^<]*)</d>"#)
        .unwrap()
        .captures_iter(xml)
        .filter_map(|c| {
            let p: Vec<&str> = c[1].split(',').collect();
            let time = p.first()?.parse::<f64>().ok().filter(|t| t.is_finite() && *t >= 0.0)?;
            // Modes 1–3 scroll, 4 bottom, 5 top; reverse (6) and scripted (7+) comments are skipped.
            let kind = match *p.get(1)? {
                "1" | "2" | "3" => Kind::Scroll,
                "4" => Kind::Bottom,
                "5" => Kind::Top,
                _ => return None,
            };
            let size = p.get(2).and_then(|s| s.parse::<f64>().ok()).filter(|s| *s > 0.0 && *s < 200.0).unwrap_or(25.0) * SCALE;
            let color = p.get(3).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0xFFFFFF) & 0xFFFFFF;
            let text = text(&c[2]);
            (!text.trim().is_empty()).then_some((time, kind, size, color, text))
        })
        .collect()
}

/// A lane is free once the previous comment has fully entered and the new one cannot catch up with it before the left edge.
fn scroll_lane(lanes: &[Option<(f64, f64)>], time: f64, width: f64) -> usize {
    let free = |last: &Option<(f64, f64)>| match *last {
        None => true,
        Some((start, w)) => {
            let speed = (WIDTH + w) / SCROLL_SECONDS;
            time >= start + (w + GAP) / speed && time + WIDTH * SCROLL_SECONDS / (WIDTH + width) >= start + SCROLL_SECONDS
        }
    };
    // When the screen is full, reuse the lane whose last comment started earliest.
    lanes.iter().position(free).unwrap_or_else(|| (0..lanes.len()).min_by(|&a, &b| lanes[a].unwrap().0.total_cmp(&lanes[b].unwrap().0)).unwrap())
}

fn fixed_lane(ends: &[f64], time: f64) -> usize {
    ends.iter().position(|&end| end <= time).unwrap_or_else(|| (0..ends.len()).min_by(|&a, &b| ends[a].total_cmp(&ends[b])).unwrap())
}

/// XML entities decoded, then made safe for one ASS line: braces and backslashes would start override tags.
fn text(raw: &str) -> String {
    let entity = Regex::new(r"&(#x[0-9a-fA-F]+|#[0-9]+|[a-z]+);").unwrap();
    let decoded = entity.replace_all(raw, |c: &Captures| {
        let name = &c[1];
        let ch = match name {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if name.starts_with("#x") => u32::from_str_radix(&name[2..], 16).ok().and_then(char::from_u32),
            _ => name[1..].parse().ok().and_then(char::from_u32),
        };
        ch.map_or_else(|| c[0].to_string(), String::from)
    });
    decoded
        .chars()
        .map(|c| match c {
            '\\' => '＼',
            '{' => '｛',
            '}' => '｝',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect()
}

fn clock(seconds: f64) -> String {
    let cs = (seconds * 100.0).round() as u64;
    format!("{}:{:02}:{:02}.{:02}", cs / 360_000, cs / 6000 % 60, cs / 100 % 60, cs % 100)
}

const HEADER: &str = "[Script Info]
ScriptType: v4.00+
PlayResX: 1920
PlayResY: 1080
WrapStyle: 2
ScaledBorderAndShadow: yes

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Danmaku,Microsoft YaHei,50,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,7,0,0,0,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
";

#[cfg(test)]
mod tests {
    use super::*;

    fn d(p: &str, text: &str) -> String {
        format!(r#"<d p="{p},1769344618,0,1c83851,1,10">{text}</d>"#)
    }

    fn lines(ass: &str) -> Vec<&str> {
        ass.lines().filter(|l| l.starts_with("Dialogue:")).collect()
    }

    #[test]
    fn disabled_or_empty_files_give_nothing() {
        let disabled = r#"<?xml version="1.0" encoding="UTF-8"?><i><chatid>1</chatid><state>2</state><text>本视频弹幕功能已被关闭</text></i>"#;
        assert_eq!(to_ass(disabled), None);
        assert_eq!(count(disabled), 0);
        assert_eq!(count(&(d("1.0,7,25,16777215", "[scripted]") + &d("2.0,1,25,16777215", "畫得出來"))), 1);
        assert_eq!(to_ass(&d("1.0,7,25,16777215", "[scripted]")), None, "scripted comments are skipped");
        assert_eq!(to_ass(&d("1.0,1,25,16777215", "  ")), None);
    }

    #[test]
    fn modes_times_colors_and_text_are_converted() {
        let xml = d("3.5,5,25,16711680", "頂部") + &d("61.25,4,36,255", "底部") + &d("0.129,1,25,16777215", r"a&lt;b&amp;{\c}&#x4E2D;");
        let ass = to_ass(&xml).unwrap();
        let l = lines(&ass);
        assert_eq!(l.len(), 3);
        assert_eq!(l[0], r"Dialogue: 0,0:00:00.13,0:00:08.13,Danmaku,,0,0,0,,{\an7\move(1920,0,-325,0)}a<b&｛＼c｝中", "sorted by time, scroll, escaped");
        assert!(l[1].starts_with(r"Dialogue: 0,0:00:03.50,0:00:07.50,Danmaku,,0,0,0,,{\an8\pos(960,0)\c&H0000FF&}頂部"), "{}", l[1]);
        assert!(l[2].starts_with(r"Dialogue: 0,0:01:01.25,0:01:05.25,Danmaku,,0,0,0,,{\an2\pos(960,1080)\fs72\c&HFF0000&}底部"), "{}", l[2]);
    }

    #[test]
    fn simultaneous_comments_get_separate_lanes() {
        let xml = d("1.0,1,25,16777215", "一樣的時間") + &d("1.0,1,25,16777215", "一樣的時間") + &d("1.0,5,25,16777215", "上") + &d("2.0,5,25,16777215", "上");
        let ass = to_ass(&xml).unwrap();
        let l = lines(&ass);
        assert!(l[0].contains(r"\move(1920,0,") && l[1].contains(r"\move(1920,54,"), "{l:?}");
        assert!(l[2].contains(r"\pos(960,0)") && l[3].contains(r"\pos(960,54)"), "{l:?}");
        // Long after both scrolled away, lane 0 is reused.
        let later = to_ass(&(xml + &d("30.0,1,25,16777215", "之後"))).unwrap();
        assert!(lines(&later)[4].contains(r"\move(1920,0,"));
    }

    #[test]
    fn full_screen_reuses_the_oldest_lane() {
        let xml: String = (0..25).map(|i| d(&format!("{:.2},1,25,16777215", 1.0 + i as f64 * 0.01), "很長的一條彈幕內容很長的一條彈幕內容")).collect();
        let ass = to_ass(&xml).unwrap();
        assert_eq!(lines(&ass).len(), 25, "nothing dropped");
        assert!(lines(&ass)[20].contains(r"\move(1920,0,"));
    }
}
