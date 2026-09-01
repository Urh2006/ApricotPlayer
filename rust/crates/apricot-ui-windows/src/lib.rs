//! Native Windows UI adapter. The first implementation milestone qualifies
//! standard Win32 controls and UI Automation with NVDA before broad screen work.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualificationGate {
    Pending,
    Passed,
    Failed,
}
