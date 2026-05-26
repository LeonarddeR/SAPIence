//! Safe iteration over `SPVTEXTFRAG` linked lists.

use windows::Win32::Media::Speech::SPVTEXTFRAG;

/// Borrowed view over a single fragment.
pub struct Fragment<'a> {
    raw: &'a SPVTEXTFRAG,
}

impl<'a> Fragment<'a> {
    pub fn raw(&self) -> &'a SPVTEXTFRAG {
        self.raw
    }

    pub fn text(&self) -> &'a [u16] {
        if self.raw.pTextStart.is_null() || self.raw.ulTextLen == 0 {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.raw.pTextStart.0, self.raw.ulTextLen as usize) }
    }

    pub fn text_string(&self) -> String {
        String::from_utf16_lossy(self.text()).to_string()
    }
}

/// # Safety
/// `ptr` must be null or point to a valid `SPVTEXTFRAG` that outlives `'a`.
pub unsafe fn from_raw<'a>(ptr: *const SPVTEXTFRAG) -> Option<Fragment<'a>> {
    unsafe { ptr.as_ref() }.map(|r| Fragment { raw: r })
}

/// # Safety
/// `ptr` must be null or point to a valid linked list of `SPVTEXTFRAG` that outlives `'a`.
pub unsafe fn iter<'a>(ptr: *const SPVTEXTFRAG) -> FragmentIter<'a> {
    FragmentIter {
        cur: unsafe { from_raw(ptr) },
    }
}

pub struct FragmentIter<'a> {
    cur: Option<Fragment<'a>>,
}

impl<'a> Iterator for FragmentIter<'a> {
    type Item = Fragment<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        let cur = self.cur.take()?;
        // SAFETY: SPVTEXTFRAG::pNext is owned by SAPI; valid for duration of Speak().
        let next = unsafe { cur.raw().pNext.as_ref() }.map(|r| Fragment { raw: r });
        let out = cur;
        self.cur = next;
        Some(out)
    }
}
