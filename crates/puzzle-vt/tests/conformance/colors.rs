//! Per-number colour conformance, through Terminal::advance rather than helpers.
//! Reference: xterm ctlseqs OSC 4/5/10–19/104/105/110–119; kitty color-stack OSC 21.
use puzzle_vt::{
    Terminal,
    color_table::{ColorSlot, ColorValue, DynamicColor},
};

/// Builds one exact OSC packet with the caller's requested wire terminator.
fn feed(term: &mut Terminal, body: &str, end: &str) {
    term.advance(format!("\x1b]{body}{end}").as_bytes());
}

/// Every dynamic number shares name parsing, query formatting and reset semantics.
fn dynamic(number: u16) {
    let role = DynamicColor::from_osc(number).expect("validated colour fixture");
    let slot = ColorSlot::Dynamic(role);
    for end in ["\x07", "\x1b\\"] {
        let mut term = Terminal::new(4, 2, 0, 0);
        let default = term.colors.get(slot);
        feed(&mut term, &format!("{number};FoReSt GrEeN"), end);
        assert_eq!(term.colors.get(slot), ColorValue::Rgb([34, 139, 34]));
        feed(&mut term, &format!("{number};?"), end);
        assert_eq!(
            std::mem::take(&mut term.replies),
            format!("\x1b]{number};rgb:2222/8b8b/2222{end}").as_bytes()
        );
        // An invalid colour must neither reset the slot nor swallow following text.
        feed(&mut term, &format!("{number};#é"), end);
        term.advance(b"X");
        assert_eq!(
            term.screen()
                .row(0)
                .expect("validated colour fixture")
                .cells[0]
                .ch,
            u32::from('X')
        );
        assert_eq!(term.colors.get(slot), ColorValue::Rgb([34, 139, 34]));
        feed(&mut term, &(number + 100).to_string(), end);
        assert_eq!(term.colors.get(slot), default);
    }
}

/// Reset-number files exercise the operation starting from an overridden slot.
fn reset(number: u16) {
    dynamic(number - 100);
}

/// Palette/special groups handle independent pairs, aliases and group resets.
fn indexed(number: u16) {
    for end in ["\x07", "\x1b\\"] {
        let mut term = Terminal::new(4, 2, 0, 0);
        feed(
            &mut term,
            &format!("{number};0;Forest Green;1;#123456;999;red;2;rgb:1/2/3"),
            end,
        );
        feed(&mut term, &format!("{number};0;?"), end);
        assert_eq!(
            std::mem::take(&mut term.replies),
            format!("\x1b]{number};0;rgb:2222/8b8b/2222{end}").as_bytes()
        );
        let slot = if number == 4 {
            ColorSlot::Palette(0)
        } else {
            ColorSlot::Special(puzzle_vt::color_table::SpecialColor::Bold)
        };
        feed(&mut term, &format!("{number};0;rgb:bad/value"), end);
        assert_eq!(term.colors.get(slot), ColorValue::Rgb([34, 139, 34]));
        feed(&mut term, &format!("{};0", number + 100), end);
        assert_eq!(term.colors.override_value(slot), None);
        // A list reset must leave other entries intact until the all-group reset.
        let other = if number == 4 {
            ColorSlot::Palette(1)
        } else {
            ColorSlot::Special(puzzle_vt::color_table::SpecialColor::Underline)
        };
        assert_eq!(term.colors.get(other), ColorValue::Rgb([0x12, 0x34, 0x56]));
        feed(&mut term, &(number + 100).to_string(), end);
        assert_eq!(term.colors.override_value(other), None);
    }
}

/// Independent OSC 10 wire contract.
#[path = "colors/osc10.rs"]
mod osc10;
/// Independent OSC 104 wire contract.
#[path = "colors/osc104.rs"]
mod osc104;
/// Independent OSC 105 wire contract.
#[path = "colors/osc105.rs"]
mod osc105;
/// Independent OSC 11 wire contract.
#[path = "colors/osc11.rs"]
mod osc11;
/// Independent OSC 110 wire contract.
#[path = "colors/osc110.rs"]
mod osc110;
/// Independent OSC 111 wire contract.
#[path = "colors/osc111.rs"]
mod osc111;
/// Independent OSC 112 wire contract.
#[path = "colors/osc112.rs"]
mod osc112;
/// Independent OSC 113 wire contract.
#[path = "colors/osc113.rs"]
mod osc113;
/// Independent OSC 114 wire contract.
#[path = "colors/osc114.rs"]
mod osc114;
/// Independent OSC 115 wire contract.
#[path = "colors/osc115.rs"]
mod osc115;
/// Independent OSC 116 wire contract.
#[path = "colors/osc116.rs"]
mod osc116;
/// Independent OSC 117 wire contract.
#[path = "colors/osc117.rs"]
mod osc117;
/// Independent OSC 118 wire contract.
#[path = "colors/osc118.rs"]
mod osc118;
/// Independent OSC 119 wire contract.
#[path = "colors/osc119.rs"]
mod osc119;
/// Independent OSC 12 wire contract.
#[path = "colors/osc12.rs"]
mod osc12;
/// Independent OSC 13 wire contract.
#[path = "colors/osc13.rs"]
mod osc13;
/// Independent OSC 14 wire contract.
#[path = "colors/osc14.rs"]
mod osc14;
/// Independent OSC 15 wire contract.
#[path = "colors/osc15.rs"]
mod osc15;
/// Independent OSC 16 wire contract.
#[path = "colors/osc16.rs"]
mod osc16;
/// Independent OSC 17 wire contract.
#[path = "colors/osc17.rs"]
mod osc17;
/// Independent OSC 18 wire contract.
#[path = "colors/osc18.rs"]
mod osc18;
/// Independent OSC 19 wire contract.
#[path = "colors/osc19.rs"]
mod osc19;
/// Ordered kitty batches, cross-family aliases and full reset.
#[path = "colors/osc21.rs"]
mod osc21;
/// Independent OSC 4 wire contract.
#[path = "colors/osc4.rs"]
mod osc4;
/// Independent OSC 5 wire contract.
#[path = "colors/osc5.rs"]
mod osc5;
