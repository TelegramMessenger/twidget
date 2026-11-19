use std::borrow::Borrow;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::rc::Rc;

#[derive(Clone)]
enum Repr {
    Owned(String),
    Shared { buf: Rc<str>, start: usize, end: usize },
}

#[derive(Clone)]
pub struct StrTendril {
    repr: Repr,
}

impl StrTendril {
    #[inline]
    pub fn new() -> Self {
        StrTendril { repr: Repr::Owned(String::new()) }
    }

    pub fn with_capacity(capacity: u32) -> Self {
        StrTendril { repr: Repr::Owned(String::with_capacity(capacity as usize)) }
    }

    #[inline]
    pub fn from_slice(s: &str) -> Self {
        StrTendril { repr: Repr::Owned(String::from(s)) }
    }

    pub fn shared(s: &str) -> Self {
        let buf: Rc<str> = Rc::from(s);
        let end = buf.len();
        StrTendril { repr: Repr::Shared { buf, start: 0, end } }
    }

    #[inline]
    pub fn from_char(c: char) -> Self {
        let mut s = String::new();
        s.push(c);
        StrTendril { repr: Repr::Owned(s) }
    }

    #[inline]
    fn as_str(&self) -> &str {
        match &self.repr {
            Repr::Owned(s) => s,
            Repr::Shared { buf, start, end } => buf.get(*start..*end).unwrap_or(""),
        }
    }

    #[inline]
    fn owned(&mut self) -> &mut String {
        if let Repr::Shared { .. } = self.repr {
            let s = String::from(self.as_str());
            self.repr = Repr::Owned(s);
        }
        match &mut self.repr {
            Repr::Owned(s) => s,
            Repr::Shared { .. } => unreachable!(),
        }
    }

    #[inline]
    pub fn len32(&self) -> u32 {
        u32::try_from(self.as_str().len()).unwrap_or(u32::MAX)
    }

    #[inline]
    pub fn push_char(&mut self, c: char) {
        self.owned().push(c);
    }

    #[inline]
    pub fn push_slice(&mut self, s: &str) {
        self.owned().push_str(s);
    }

    pub fn push_tendril(&mut self, other: &StrTendril) {
        if self.is_empty() {
            *self = other.clone();
        } else {
            self.owned().push_str(other.as_str());
        }
    }

    pub fn clear(&mut self) {
        match &mut self.repr {
            Repr::Owned(s) => s.clear(),
            Repr::Shared { .. } => self.repr = Repr::Owned(String::new()),
        }
    }

    pub fn pop_front(&mut self, n: u32) {
        let n = n as usize;
        match &mut self.repr {
            Repr::Owned(s) => {
                let n = n.min(s.len());
                if s.is_char_boundary(n) {
                    s.drain(..n);
                } else {
                    s.clear();
                }
            }
            Repr::Shared { buf, start, end } => {
                let next = (*start + n).min(*end);
                if buf.is_char_boundary(next) {
                    *start = next;
                } else {
                    *start = *end;
                }
            }
        }
    }

    pub fn pop_front_char(&mut self) -> Option<char> {
        let c = self.as_str().chars().next()?;
        self.pop_front(c.len_utf8() as u32);
        Some(c)
    }

    pub fn pop_front_char_run<F: Fn(char) -> bool>(&mut self, classify: F) -> Option<(StrTendril, bool)> {
        let mut chars = self.as_str().char_indices();
        let (_, first) = chars.next()?;
        let class = classify(first);
        let end = chars.find(|&(_, c)| classify(c) != class).map(|(i, _)| i).unwrap_or(self.as_str().len());
        let run = self.subtendril(0, end as u32);
        self.pop_front(end as u32);
        Some((run, class))
    }

    pub fn pop_back(&mut self, n: u32) {
        let n = n as usize;
        match &mut self.repr {
            Repr::Owned(s) => {
                let keep = s.len().saturating_sub(n);
                if s.is_char_boundary(keep) {
                    s.truncate(keep);
                } else {
                    s.clear();
                }
            }
            Repr::Shared { buf, start, end } => {
                let keep = end.saturating_sub(n).max(*start);
                if buf.is_char_boundary(keep) {
                    *end = keep;
                } else {
                    *end = *start;
                }
            }
        }
    }

    pub fn subtendril(&self, offset: u32, length: u32) -> StrTendril {
        let offset = offset as usize;
        let length = length as usize;
        match &self.repr {
            Repr::Shared { buf, start, end } => {
                let from = (*start + offset).min(*end);
                let to = (from + length).min(*end);
                if buf.is_char_boundary(from) && buf.is_char_boundary(to) {
                    StrTendril { repr: Repr::Shared { buf: buf.clone(), start: from, end: to } }
                } else {
                    StrTendril::new()
                }
            }
            Repr::Owned(s) => {
                let from = offset.min(s.len());
                let to = (from + length).min(s.len());
                StrTendril::from_slice(s.get(from..to).unwrap_or(""))
            }
        }
    }

    pub fn try_reserve(&mut self, additional: u32) -> Result<(), ()> {
        self.owned().try_reserve(additional as usize).map_err(|_| ())
    }
}

impl Default for StrTendril {
    fn default() -> Self {
        StrTendril::new()
    }
}

impl Deref for StrTendril {
    type Target = str;
    #[inline]
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for StrTendril {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for StrTendril {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl PartialEq for StrTendril {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for StrTendril {}

impl PartialEq<str> for StrTendril {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl Hash for StrTendril {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state)
    }
}

impl<'a> From<&'a str> for StrTendril {
    fn from(s: &'a str) -> Self {
        StrTendril::from_slice(s)
    }
}

impl From<String> for StrTendril {
    fn from(s: String) -> Self {
        StrTendril { repr: Repr::Owned(s) }
    }
}

impl From<StrTendril> for String {
    fn from(t: StrTendril) -> String {
        match t.repr {
            Repr::Owned(s) => s,
            Repr::Shared { .. } => String::from(t.as_str()),
        }
    }
}

impl fmt::Display for StrTendril {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_str(), f)
    }
}

impl fmt::Debug for StrTendril {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Write for StrTendril {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.push_slice(s);
        Ok(())
    }
}

pub trait SliceExt {
    fn to_tendril(&self) -> StrTendril;
}

impl SliceExt for str {
    fn to_tendril(&self) -> StrTendril {
        StrTendril::from_slice(self)
    }
}

impl PartialOrd for StrTendril {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for StrTendril {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::StrTendril;

    #[test]
    fn shared_slices_share_and_bound() {
        let mut t = StrTendril::shared("héllo wörld");
        let head = t.subtendril(0, 6);
        assert_eq!(&*head, "héllo");
        t.pop_front(7);
        assert_eq!(&*t, "wörld");
        assert_eq!(&*t.subtendril(100, 5), "");
        assert_eq!(&*t.subtendril(0, 2), "");
        t.pop_front(1);
        assert_eq!(&*t, "örld");
        t.pop_front(1);
        assert_eq!(&*t, "");
    }

    #[test]
    fn owned_edits() {
        let mut t = StrTendril::from_slice("ab");
        t.push_char('ü');
        t.push_slice("cd");
        t.push_tendril(&StrTendril::shared("ef"));
        assert_eq!(&*t, "abücdef");
        t.pop_back(2);
        assert_eq!(&*t, "abücd");
        t.pop_back(3);
        assert_eq!(&*t, "");
        let mut empty = StrTendril::new();
        empty.push_tendril(&StrTendril::shared("xy"));
        empty.push_char('z');
        assert_eq!(&*empty, "xyz");
        assert_eq!(empty.len32(), 3);
    }

    #[test]
    fn char_runs() {
        let mut t = StrTendril::shared("  \tab c");
        assert_eq!(t.pop_front_char_run(|c| c.is_ascii_whitespace()).map(|(s, w)| (String::from(s), w)), Some(("  \t".into(), true)));
        assert_eq!(t.pop_front_char_run(|c| c.is_ascii_whitespace()).map(|(s, w)| (String::from(s), w)), Some(("ab".into(), false)));
        assert_eq!(t.pop_front_char(), Some(' '));
        assert_eq!(t.pop_front_char(), Some('c'));
        assert_eq!(t.pop_front_char(), None);
        assert!(t.pop_front_char_run(|c| c == 'x').is_none());
    }
}
