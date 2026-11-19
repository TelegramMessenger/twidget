use std::borrow::{Borrow, Cow};
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::ops::Deref;
use std::rc::Rc;

pub trait StaticAtomSet: 'static + Clone + PartialEq + Eq {
    const STRINGS: &'static [&'static str];
    fn index(s: &str) -> Option<u32>;
}

#[derive(Clone, PartialEq, Eq)]
enum Repr {
    Static(u32),
    Dynamic(Rc<str>),
}

#[derive(Clone, PartialEq, Eq)]
pub struct Atom<S> {
    repr: Repr,
    set: PhantomData<S>,
}

impl<S> Atom<S> {
    pub const fn pack_static(index: u32) -> Self {
        Atom { repr: Repr::Static(index), set: PhantomData }
    }
}

impl<S: StaticAtomSet> Atom<S> {
    #[inline]
    fn text(&self) -> &str {
        match &self.repr {
            Repr::Static(i) => S::STRINGS.get(*i as usize).copied().unwrap_or(""),
            Repr::Dynamic(s) => s,
        }
    }

    pub fn eq_ignore_ascii_case(&self, other: &Self) -> bool {
        self == other || self.text().eq_ignore_ascii_case(other.text())
    }

    pub fn eq_str_ignore_ascii_case(&self, other: &str) -> bool {
        self.text().eq_ignore_ascii_case(other)
    }

    pub fn to_ascii_lowercase(&self) -> Self {
        let text = self.text();
        if text.bytes().any(|b| b.is_ascii_uppercase()) {
            Self::from(text.to_ascii_lowercase())
        } else {
            self.clone()
        }
    }

    pub fn to_ascii_uppercase(&self) -> Self {
        let text = self.text();
        if text.bytes().any(|b| b.is_ascii_lowercase()) {
            Self::from(text.to_ascii_uppercase())
        } else {
            self.clone()
        }
    }
}

impl<S: StaticAtomSet> Default for Atom<S> {
    fn default() -> Self {
        Self::from("")
    }
}

impl<S: StaticAtomSet> Deref for Atom<S> {
    type Target = str;
    #[inline]
    fn deref(&self) -> &str {
        self.text()
    }
}

impl<S: StaticAtomSet> AsRef<str> for Atom<S> {
    fn as_ref(&self) -> &str {
        self.text()
    }
}

impl<S: StaticAtomSet> Borrow<str> for Atom<S> {
    fn borrow(&self) -> &str {
        self.text()
    }
}

impl<S: StaticAtomSet> Hash for Atom<S> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text().hash(state)
    }
}

impl<S: StaticAtomSet> PartialOrd for Atom<S> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<S: StaticAtomSet> Ord for Atom<S> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.text().cmp(other.text())
    }
}

impl<S: StaticAtomSet> PartialEq<str> for Atom<S> {
    fn eq(&self, other: &str) -> bool {
        self.text() == other
    }
}

impl<S: StaticAtomSet> PartialEq<&str> for Atom<S> {
    fn eq(&self, other: &&str) -> bool {
        self.text() == *other
    }
}

impl<'a, S: StaticAtomSet> From<&'a str> for Atom<S> {
    #[inline]
    fn from(s: &'a str) -> Self {
        match S::index(s) {
            Some(i) => Self::pack_static(i),
            None => Atom { repr: Repr::Dynamic(Rc::from(s)), set: PhantomData },
        }
    }
}

impl<S: StaticAtomSet> From<String> for Atom<S> {
    fn from(s: String) -> Self {
        match S::index(&s) {
            Some(i) => Self::pack_static(i),
            None => Atom { repr: Repr::Dynamic(Rc::from(s)), set: PhantomData },
        }
    }
}

impl<'a, S: StaticAtomSet> From<Cow<'a, str>> for Atom<S> {
    fn from(s: Cow<'a, str>) -> Self {
        match s {
            Cow::Borrowed(s) => Self::from(s),
            Cow::Owned(s) => Self::from(s),
        }
    }
}

impl<S: StaticAtomSet> fmt::Display for Atom<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.text(), f)
    }
}

impl<S: StaticAtomSet> fmt::Debug for Atom<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.text(), f)
    }
}

#[cfg(test)]
mod tests {
    use crate::{local_name, ns, LocalName, Namespace};

    #[test]
    fn static_and_dynamic_atoms() {
        let div = LocalName::from("div");
        assert_eq!(div, local_name!("div"));
        assert!(matches!(div, local_name!("div")));
        let custom = LocalName::from("x-custom-tag");
        assert_eq!(&*custom, "x-custom-tag");
        assert_eq!(custom, LocalName::from(String::from("x-custom-tag")));
        assert_ne!(custom, div);
        assert_eq!(LocalName::from("DIV").to_ascii_lowercase(), local_name!("div"));
        assert!(LocalName::from("Div").eq_str_ignore_ascii_case("dIV"));
        assert_eq!(Namespace::from("http://www.w3.org/1999/xhtml"), ns!(html));
        assert_eq!(&*ns!(svg), "http://www.w3.org/2000/svg");
        assert_eq!(&*LocalName::from(""), "");
    }

    #[test]
    fn entities() {
        let table = &crate::data::NAMED_ENTITIES;
        assert_eq!(table.get("gt;"), Some(&(62, 0)));
        assert_eq!(table.get("acE;"), Some(&(8766, 819)));
        assert_eq!(table.get("ac"), Some(&(0, 0)));
        assert_eq!(table.get(""), Some(&(0, 0)));
        assert_eq!(table.get("notanentity;"), None);
    }
}
