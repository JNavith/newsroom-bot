use ecow::EcoString;
use std::{cmp::Ordering, iter::zip};

#[derive(Debug, Clone)]
pub struct CaseInsensitive<'a>(pub &'a [u8]);

impl<'a> Ord for CaseInsensitive<'a> {
    fn cmp(&self, other: &Self) -> Ordering {
        for (s, o) in zip(self.0, other.0) {
            match s.to_ascii_lowercase().cmp(&o.to_ascii_lowercase()) {
                Ordering::Less => return Ordering::Less,
                Ordering::Greater => return Ordering::Greater,
                Ordering::Equal => {}
            }
        }

        match self.0.len().cmp(&other.0.len()) {
            Ordering::Less => Ordering::Less,
            Ordering::Equal => Ordering::Equal,
            Ordering::Greater => Ordering::Greater,
        }
    }
}

impl<'a> PartialOrd for CaseInsensitive<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a> PartialEq for CaseInsensitive<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl<'a> Eq for CaseInsensitive<'a> {}

#[derive(Debug, Clone)]
pub struct CaseInsensitiveString(pub EcoString);

impl PartialEq for CaseInsensitiveString {
    fn eq(&self, other: &Self) -> bool {
        CaseInsensitive(self.0.as_bytes()) == CaseInsensitive(other.0.as_bytes())
    }
}

impl PartialOrd for CaseInsensitiveString {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        CaseInsensitive(self.0.as_bytes()).partial_cmp(&CaseInsensitive(other.0.as_bytes()))
    }
}

impl Eq for CaseInsensitiveString {}

impl Ord for CaseInsensitiveString {
    fn cmp(&self, other: &Self) -> Ordering {
        CaseInsensitive(self.0.as_bytes()).cmp(&CaseInsensitive(other.0.as_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proptest::proptest;

    use crate::case_insensitive::CaseInsensitiveString;

    #[test]
    fn test_should_be_equal() {
        assert_eq!(
            CaseInsensitiveString("woRLD".into()),
            CaseInsensitiveString("World".into())
        );
    }

    proptest! {
        // Cause remember, the reason I wrote this is to be more efficient than it
        #[test]
        fn test_should_have_same_behavior_as_using_builtin_method(a in "\\PC*", b in "\\PC*") {
            let std_impl = a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase());
            let my_impl = CaseInsensitiveString(a.into()).cmp(&CaseInsensitiveString(b.into()));

            assert_eq!(std_impl, my_impl);
        }
    }

    #[test]
    fn can_be_used_in_btree_map() {
        let btreemap = BTreeMap::from_iter([
            (CaseInsensitiveString("Hello".into()), 127),
            (CaseInsensitiveString("everyBODY".into()), 32),
        ]);

        assert_eq!(
            btreemap.get(&CaseInsensitiveString("heLLO".into())),
            Some(&127)
        );

        assert_eq!(
            btreemap.get(&CaseInsensitiveString("EVERYbody".into())),
            Some(&32)
        );
    }
}
