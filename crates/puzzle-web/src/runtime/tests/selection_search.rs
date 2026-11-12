//! Public selections and searches must paint the submitted browser canvas.
//! Cell paper assertions are independent of the engine's own computed ranges.
use super::viewport::{pointer, rendered, write};
use super::*;

/// Every selection operation and its convenience fact use the public facade.
#[wasm_bindgen_test(async)]
async fn selection_api_members_and_selection_event() {
    let (t, host) = rendered().await;
    write(&t, "alpha beta\r\nabcdefghijklmnopqrstUV\r\n\x1b]133;A\x07prompt\x1b]133;B\x07cmd\r\n\x1b]133;C\x07output\r\n\x1b]133;D;0\x07").await;
    let first = t
        .owner
        .borrow()
        .session
        .buffer()
        .line_ids()
        .unwrap()
        .start()
        .0;
    let selection = t.selection().unwrap();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    let cb = Closure::wrap(
        Box::new(move |v: JsValue| sink.borrow_mut().push(v)) as Box<dyn FnMut(JsValue)>
    );
    let handle = t
        .on(
            "selectionChange".into(),
            cb.as_ref().unchecked_ref::<Function>().clone(),
        )
        .unwrap();
    selection.select(range(first, 0, first, 4), None).unwrap();
    assert_eq!(selection.text().unwrap(), "alpha");
    let end = value::get(&selection.range().unwrap(), "end").unwrap();
    assert_eq!(value::get(&end, "col").unwrap().as_f64(), Some(4.0));
    selection
        .select(range(first, 7, first, 7), Some("word".into()))
        .unwrap();
    assert_eq!(selection.text().unwrap(), "beta");
    selection.select_line((first as f64 + 1.0).into()).unwrap();
    assert_eq!(selection.text().unwrap(), "abcdefghijklmnopqrstUV");
    zone_and_all(&t, &selection);
    selection.clear().unwrap();
    assert!(selection.range().unwrap().is_undefined());
    assert!(selection.text().unwrap().is_empty());
    assert!(seen.borrow().last().unwrap().is_undefined());
    let count = seen.borrow().len();
    handle.dispose();
    selection.select_all().unwrap();
    assert_eq!(seen.borrow().len(), count);
    assert!(
        selection
            .select(range(first, 0, first, 1), Some("bad".into()))
            .is_err()
    );
    t.dispose().unwrap();
    assert!(selection.text().is_err());
    host.remove();
}
/// Range inputs use the documented Position names, not internal field spellings.
fn range(first: u64, start: u32, last: u64, end: u32) -> JsValue {
    js_sys::JSON::parse(&format!(
        r#"{{"start":{{"lineId":{first},"col":{start}}},"end":{{"lineId":{last},"col":{end}}}}}"#
    ))
    .unwrap()
}
/// Shared pointer gestures preserve click expansion, block and Shift extension.
#[wasm_bindgen_test(async)]
async fn pointer_selection_and_copy_in_key_handler() {
    let (t, host) = rendered().await;
    write(&t, "alpha beta\r\nABCDEFGHIJ\r\nabcdefghij").await;
    let selection = t.selection().unwrap();
    pointer(&t, "pointerdown", 0.5, 0.5, 1, 0, false);
    pointer(&t, "pointermove", 2.5, 0.5, 1, 0, false);
    pointer(&t, "pointerup", 4.5, 0.5, 1, 0, false);
    assert_eq!(selection.text().unwrap(), "alpha");
    // Trusted WebDriver actions cover word and logical-line expansion separately.
    pointer(
        &t,
        "pointerdown",
        1.5,
        1.5,
        1,
        puzzle_term::vt::input::Modifiers::ALT,
        false,
    );
    pointer(
        &t,
        "pointerup",
        3.5,
        2.5,
        1,
        puzzle_term::vt::input::Modifiers::ALT,
        false,
    );
    assert_eq!(selection.text().unwrap(), "BCD\nbcd\n");
    pointer(
        &t,
        "pointerdown",
        5.5,
        2.5,
        1,
        puzzle_term::vt::input::Modifiers::SHIFT,
        false,
    );
    pointer(
        &t,
        "pointerup",
        5.5,
        2.5,
        1,
        puzzle_term::vt::input::Modifiers::SHIFT,
        false,
    );
    assert!(selection.text().unwrap().contains("bcdef"));
    clipboard_key(&t, &selection.text().unwrap());
    pointer_tracking_override(&t).await;
    t.dispose().unwrap();
    host.remove();
}
/// Replace only the browser service: real keydown, keybinding, Copy and policy run.
fn clipboard_key(t: &PuzzleTerminal, expected: &str) {
    let clipboard = window().unwrap().navigator().clipboard();
    let original = value::get(clipboard.as_ref(), "writeText").unwrap();
    let inside = Rc::new(std::cell::Cell::new(false));
    let flag = inside.clone();
    let copied = Rc::new(RefCell::new(Vec::new()));
    let sink = copied.clone();
    let cb = Closure::wrap(Box::new(move |text: String| {
        assert!(flag.get(), "copy escaped the keydown user-activation turn");
        sink.borrow_mut().push(text);
        Promise::resolve(&JsValue::UNDEFINED)
    }) as Box<dyn FnMut(String) -> Promise>);
    js_sys::Reflect::set(clipboard.as_ref(), &"writeText".into(), cb.as_ref()).unwrap();
    let init = web_sys::KeyboardEventInit::new();
    init.set_key("c");
    init.set_code("KeyC");
    if crate::dom::platform() == puzzle_term::keybind::Platform::Mac {
        init.set_meta_key(true);
    } else {
        init.set_ctrl_key(true);
        init.set_shift_key(true);
    }
    let area = t.owner.borrow().dom.as_ref().unwrap().textarea().clone();
    inside.set(true);
    area.dispatch_event(
        &web_sys::KeyboardEvent::new_with_keyboard_event_init_dict("keydown", &init).unwrap(),
    )
    .unwrap();
    inside.set(false);
    js_sys::Reflect::set(clipboard.as_ref(), &"writeText".into(), &original).unwrap();
    assert_eq!(copied.borrow().as_slice(), [expected]);
}
/// Search getters update after frames; every operation and the request event are public.
#[wasm_bindgen_test(async)]
async fn search_api_members_progress_and_requested_event() {
    let (t, host) = rendered().await;
    write(&t, "alpha ALPHA alpha").await;
    let search = t.search().unwrap();
    let progress = search.find("alpha".into(), None).unwrap();
    assert!(!progress.complete().unwrap());
    assert_eq!(progress.count().unwrap(), 0);
    let _ = presentation::presented_pixels(&t).await;
    assert!(progress.complete().unwrap());
    assert_eq!(progress.count().unwrap(), 3);
    assert_eq!(search.next().unwrap().current().unwrap(), Some(0));
    assert_eq!(search.next().unwrap().current().unwrap(), Some(1));
    assert_eq!(search.prev().unwrap().current().unwrap(), Some(0));
    let options = js_sys::JSON::parse(r#"{"caseSensitive":true}"#).unwrap();
    let exact = search.find("alpha".into(), Some(options)).unwrap();
    let _ = presentation::presented_pixels(&t).await;
    assert_eq!(exact.count().unwrap(), 2);
    search.clear().unwrap();
    assert_eq!(progress.count().unwrap(), 0);
    assert!(
        search
            .find(
                "x".into(),
                Some(js_sys::JSON::parse(r#"{"regex":true}"#).unwrap())
            )
            .is_err()
    );
    let seen = Rc::new(std::cell::Cell::new(0));
    let sink = seen.clone();
    let cb = Closure::wrap(
        Box::new(move |_: JsValue| sink.set(sink.get() + 1)) as Box<dyn FnMut(JsValue)>
    );
    let handle = t
        .on(
            "searchRequested".into(),
            cb.as_ref().unchecked_ref::<Function>().clone(),
        )
        .unwrap();
    let spec = js_sys::JSON::parse(
        if crate::dom::platform() == puzzle_term::keybind::Platform::Mac {
            r#"{"key":"f","meta":true}"#
        } else {
            r#"{"key":"f","ctrl":true,"shift":true}"#
        },
    )
    .unwrap();
    t.send_key(spec.clone()).unwrap();
    assert_eq!(seen.get(), 1);
    handle.dispose();
    t.send_key(spec).unwrap();
    assert_eq!(seen.get(), 1);
    t.dispose().unwrap();
    assert!(search.clear().is_err());
    host.remove();
}
/// Known cells must have exact authored paper and individual glyph ink at both DPRs.
#[wasm_bindgen_test(async)]
async fn public_selection_and_search_fill_exact_cells_with_served_assets() {
    let (t, host) = rendered().await;
    // Dispose before a wasm assertion trap so later resource tests remain independent.
    let result = authored_fills(&t).await;
    t.dispose().unwrap();
    host.remove();
    assert!(result.is_ok(), "{}", result.unwrap_err());
}
/// Authored per-cell paper/ink checks return evidence before the owner is released.
async fn authored_fills(t: &PuzzleTerminal) -> Result<(), String> {
    write(&t, "\x1b[?25lalpha beta alpha\r\nABCDEFGHIJKLMNOPQRSTUV").await;
    let baseline = presentation::presented_pixels(&t).await;
    let first = t
        .owner
        .borrow()
        .session
        .buffer()
        .line_ids()
        .unwrap()
        .start()
        .0;
    t.selection()
        .unwrap()
        .select(range(first, 0, first, 4), None)
        .unwrap();
    let selected = presentation::presented_pixels(&t).await;
    paper_cells(&t, &baseline, &selected, &[(0, 0, 5, [34, 85, 102])])?;
    t.selection().unwrap().clear().unwrap();
    let search = t.search().unwrap();
    search.find("alpha".into(), None).unwrap();
    let matches = presentation::presented_pixels(&t).await;
    paper_cells(
        &t,
        &baseline,
        &matches,
        &[(0, 0, 5, [51, 68, 85]), (0, 11, 16, [51, 68, 85])],
    )?;
    search.next().unwrap();
    let current = presentation::presented_pixels(&t).await;
    paper_cells(
        &t,
        &baseline,
        &current,
        &[(0, 0, 5, [119, 85, 51]), (0, 11, 16, [51, 68, 85])],
    )?;
    search.clear().unwrap();
    if presentation::presented_pixels(&t).await != baseline {
        return Err("clear did not restore the authored baseline".into());
    }
    Ok(())
}
/// Every outside pixel is identical; fill alone cannot satisfy a per-cell ink guard.
fn paper_cells(
    t: &PuzzleTerminal,
    baseline: &[u8],
    pixels: &[u8],
    spans: &[(u32, u32, u32, [u8; 3])],
) -> Result<(), String> {
    let r = t.owner.borrow();
    let m = r.session.cell_metrics().unwrap();
    let width = r.dom.as_ref().unwrap().canvas().width();
    let padding = (r.settings.render_padding * r.dpr as f32).round() as u32;
    let mut paper = vec![
        0;
        spans
            .iter()
            .map(|(_, a, b, _)| (b - a) as usize)
            .sum::<usize>()
    ];
    let mut ink = paper.clone();
    for (at, (before, after)) in baseline
        .chunks_exact(4)
        .zip(pixels.chunks_exact(4))
        .enumerate()
    {
        let x = at as u32 % width;
        let y = at as u32 / width;
        let mut cell = None;
        let mut offset = 0;
        for &(row, a, b, color) in spans {
            if x >= padding + a * m.width_px
                && x < padding + b * m.width_px
                && y >= padding + row * m.height_px
                && y < padding + (row + 1) * m.height_px
            {
                cell = Some((offset + ((x - padding) / m.width_px - a) as usize, color));
                break;
            }
            offset += (b - a) as usize;
        }
        if let Some((index, color)) = cell {
            if after[..3] == color {
                paper[index] += 1;
            } else if after[..3].iter().zip(color).any(|(a, b)| a.abs_diff(b) > 8) {
                ink[index] += 1;
            }
        } else {
            if before != after {
                return Err(format!(
                    "unexpected fill at {x},{y}: {before:?} -> {after:?}; spans={spans:?}; cell={}x{}, padding={padding}",
                    m.width_px, m.height_px
                ));
            }
        }
    }
    for (paper, ink) in paper.into_iter().zip(ink) {
        if paper <= 20 || ink < 3 {
            return Err(format!("cell paper={paper}, ink={ink}"));
        }
    }
    Ok(())
}

/// Whole-buffer and semantic selection must not choose a different command on missing ID.
fn zone_and_all(t: &PuzzleTerminal, selection: &super::super::api_selection::PuzzleSelection) {
    selection.select_all().unwrap();
    assert!(selection.text().unwrap().contains("alpha beta"));
    let zone = t
        .owner
        .borrow()
        .session
        .buffer()
        .zones()
        .find(|m| m.kind == puzzle_term::vt::zones::ZoneKind::Output)
        .unwrap()
        .id;
    selection.select_zone_output((zone as f64).into()).unwrap();
    assert_eq!(selection.text().unwrap(), "output\n");
    selection.select_zone_output(99999.into()).unwrap();
    assert_eq!(selection.text().unwrap(), "output\n");
}

/// Live tracking claims ordinary clicks while Shift restores local expansion.
async fn pointer_tracking_override(t: &PuzzleTerminal) {
    let selection = t.selection().unwrap();
    // Tracking sends clicks to the application; Shift restores local selection.
    write(&t, "\x1b[?1000;1006h").await;
    let before = selection.text().unwrap();
    pointer(&t, "pointerdown", 0.5, 0.5, 1, 0, false);
    pointer(&t, "pointerup", 0.5, 0.5, 1, 0, false);
    assert_eq!(selection.text().unwrap(), before);
    // Shift extends an existing range; clear it to test a fresh local drag.
    selection.clear().unwrap();
    pointer(
        &t,
        "pointerdown",
        0.5,
        0.5,
        1,
        puzzle_term::vt::input::Modifiers::SHIFT,
        false,
    );
    pointer(
        &t,
        "pointerup",
        4.5,
        0.5,
        1,
        puzzle_term::vt::input::Modifiers::SHIFT,
        false,
    );
    assert!(selection.text().unwrap().starts_with("alpha"));
}
