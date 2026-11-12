//! Cluster presentation selection before font fallback (design §8.4, D-31).
//!
//! Selectors stay in the source string for shaping; they only change which
//! source is tried first. No selector or joiner receives a separate lookup.
//! Emoji_Presentation is a Unicode property, independent of cell width:
//! CJK has width two as well, but must retain ordinary text fallback.

/// The requested glyph appearance, also part of the fallback cache key.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Presentation {
    /// Try text faces before the emoji source (including explicit VS15).
    Text,
    /// Try the emoji source first (VS16 or default emoji presentation).
    Emoji,
}

/// Resolves the last selector, otherwise the first drawable scalar's property.
pub fn presentation(cluster: &str) -> Presentation {
    // Like the VT selector path, the last selector wins when malformed
    // input repeats them. They remain invisible even in such a cluster.
    if let Some(selector) = cluster
        .chars()
        .rev()
        .find(|ch| matches!(ch, '\u{fe0e}' | '\u{fe0f}'))
    {
        return if selector == '\u{fe0f}' {
            Presentation::Emoji
        } else {
            Presentation::Text
        };
    }
    let emoji = cluster
        .chars()
        .find(|ch| !crate::font_set::is_default_ignorable(*ch))
        .is_some_and(emoji_presentation);
    if emoji {
        Presentation::Emoji
    } else {
        Presentation::Text
    }
}

/// Tests Unicode 17 Emoji_Presentation without a platform font-name heuristic.
fn emoji_presentation(ch: char) -> bool {
    // Sorted disjoint property intervals allow one logarithmic lookup.
    // Source: https://www.unicode.org/Public/17.0.0/ucd/emoji/emoji-data.txt.
    let scalar = ch as u32;
    let index = EMOJI_PRESENTATION.partition_point(|(_, end)| *end < scalar);
    EMOJI_PRESENTATION
        .get(index)
        .is_some_and(|(start, _)| *start <= scalar)
}

/// Unicode 17 default emoji intervals; unlisted scalars have the Text default.
const EMOJI_PRESENTATION: &[(u32, u32)] = &[
    // watch..hourglass done.
    (0x231a, 0x231b),
    // fast-forward button..fast down button.
    (0x23e9, 0x23ec),
    // alarm clock.
    (0x23f0, 0x23f0),
    // hourglass not done.
    (0x23f3, 0x23f3),
    // white medium-small square..black medium-small square.
    (0x25fd, 0x25fe),
    // umbrella with rain drops..hot beverage.
    (0x2614, 0x2615),
    // Aries..Pisces.
    (0x2648, 0x2653),
    // wheelchair symbol.
    (0x267f, 0x267f),
    // anchor.
    (0x2693, 0x2693),
    // high voltage.
    (0x26a1, 0x26a1),
    // white circle..black circle.
    (0x26aa, 0x26ab),
    // soccer ball..baseball.
    (0x26bd, 0x26be),
    // snowman without snow..sun behind cloud.
    (0x26c4, 0x26c5),
    // Ophiuchus.
    (0x26ce, 0x26ce),
    // no entry.
    (0x26d4, 0x26d4),
    // church.
    (0x26ea, 0x26ea),
    // fountain..flag in hole.
    (0x26f2, 0x26f3),
    // sailboat.
    (0x26f5, 0x26f5),
    // tent.
    (0x26fa, 0x26fa),
    // fuel pump.
    (0x26fd, 0x26fd),
    // check mark button.
    (0x2705, 0x2705),
    // raised fist..raised hand.
    (0x270a, 0x270b),
    // sparkles.
    (0x2728, 0x2728),
    // cross mark.
    (0x274c, 0x274c),
    // cross mark button.
    (0x274e, 0x274e),
    // red question mark..white exclamation mark.
    (0x2753, 0x2755),
    // red exclamation mark.
    (0x2757, 0x2757),
    // plus..divide.
    (0x2795, 0x2797),
    // curly loop.
    (0x27b0, 0x27b0),
    // double curly loop.
    (0x27bf, 0x27bf),
    // black large square..white large square.
    (0x2b1b, 0x2b1c),
    // star.
    (0x2b50, 0x2b50),
    // hollow red circle.
    (0x2b55, 0x2b55),
    // mahjong red dragon.
    (0x1f004, 0x1f004),
    // joker.
    (0x1f0cf, 0x1f0cf),
    // AB button (blood type).
    (0x1f18e, 0x1f18e),
    // CL button..VS button.
    (0x1f191, 0x1f19a),
    // regional indicator symbol letter a..regional indicator symbol letter z.
    (0x1f1e6, 0x1f1ff),
    // Japanese “here” button.
    (0x1f201, 0x1f201),
    // Japanese “free of charge” button.
    (0x1f21a, 0x1f21a),
    // Japanese “reserved” button.
    (0x1f22f, 0x1f22f),
    // Japanese “prohibited” button..Japanese “not free of charge” button.
    (0x1f232, 0x1f236),
    // Japanese “application” button..Japanese “open for business” button.
    (0x1f238, 0x1f23a),
    // Japanese “bargain” button..Japanese “acceptable” button.
    (0x1f250, 0x1f251),
    // cyclone..milky way.
    (0x1f300, 0x1f320),
    // hot dog..burrito.
    (0x1f32d, 0x1f335),
    // tulip..tangerine.
    (0x1f337, 0x1f37c),
    // bottle with popping cork..popcorn.
    (0x1f37e, 0x1f393),
    // carousel horse..person surfing.
    (0x1f3a0, 0x1f3ca),
    // cricket game..ping pong.
    (0x1f3cf, 0x1f3d3),
    // house..Japanese post office.
    (0x1f3e0, 0x1f3f0),
    // black flag.
    (0x1f3f4, 0x1f3f4),
    // badminton..rabbit.
    (0x1f3f8, 0x1f43e),
    // eyes.
    (0x1f440, 0x1f440),
    // ear..bust in silhouette.
    (0x1f442, 0x1f4fc),
    // prayer beads..repeat single button.
    (0x1f4ff, 0x1f53d),
    // kaaba..menorah.
    (0x1f54b, 0x1f54e),
    // one o’clock..twelve o’clock.
    (0x1f550, 0x1f567),
    // man dancing.
    (0x1f57a, 0x1f57a),
    // middle finger..vulcan salute.
    (0x1f595, 0x1f596),
    // black heart.
    (0x1f5a4, 0x1f5a4),
    // mount fuji..moai.
    (0x1f5fb, 0x1f64f),
    // rocket.
    (0x1f680, 0x1f6c5),
    // person in bed.
    (0x1f6cc, 0x1f6cc),
    // place of worship.
    (0x1f6d0, 0x1f6d2),
    // hindu temple.
    (0x1f6d5, 0x1f6d8),
    // wireless.
    (0x1f6dc, 0x1f6df),
    // airplane departure..airplane arrival.
    (0x1f6eb, 0x1f6ec),
    // kick scooter..canoe.
    (0x1f6f4, 0x1f6fc),
    // orange circle..brown square.
    (0x1f7e0, 0x1f7eb),
    // heavy equals sign.
    (0x1f7f0, 0x1f7f0),
    // pinched fingers.
    (0x1f90c, 0x1f93a),
    // people wrestling..person playing handball.
    (0x1f93c, 0x1f945),
    // 1st place medal..martial arts uniform.
    (0x1f947, 0x1f9ff),
    // ballet shoes..shorts.
    (0x1fa70, 0x1fa7c),
    // yo-yo..parachute.
    (0x1fa80, 0x1fa8a),
    // treasure chest.
    (0x1fa8e, 0x1fac6),
    // hairy creature.
    (0x1fac8, 0x1fac8),
    // orca.
    (0x1facd, 0x1fadc),
    // splatter.
    (0x1fadf, 0x1faea),
    // fight cloud.
    (0x1faef, 0x1faf8),
];

/// D-31 width policy is independent of presentation source choice.
pub(crate) mod width;
