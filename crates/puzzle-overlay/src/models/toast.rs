//! Immutable admitted notifications shared with the harness toast painter.
//! These values are copied only when delivery/interaction changes the model.
/// A toast is a view of Session-owned notification state, never a second queue.
#[derive(Clone, Debug)]
pub struct Toast {
    /// Never-reused owner-local identity used in presented hit regions.
    pub serial: u64,
    /// Control-stripped, capped title.
    pub title: String,
    /// Control-stripped, capped body.
    pub body: String,
    /// Protocol urgency 0/1/2 selects existing semantic theme tokens.
    pub urgency: u8,
}
