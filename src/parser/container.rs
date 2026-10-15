use std::ops::Not;

use super::{Parser, MAX_ALLOWED_DEPTH};
use crate::{error::ErrorCode::*, error::Result, reader::Reader, value::visitor::JsonVisitor};

#[cfg(test)]
mod tests;

struct Ignored;
impl<'de> JsonVisitor<'de> for Ignored {}

impl<'de, R: Reader<'de>> Parser<R> {
    pub(super) fn skip_container_checked(&mut self, first: u8) -> Result<()> {
        self.walk_value::<_, true>(Some(first), &mut Ignored, None)
    }

    pub(super) fn walk_value<V: JsonVisitor<'de>, const SKIP: bool>(
        &mut self,
        mut first: Option<u8>,
        vis: &mut V,
        mut strbuf: Option<&mut Vec<u8>>,
    ) -> Result<()> {
        // The existing Serde budget also bounds these nonrecursive paths.
        let mut parents = [(0_u8, 0_usize); MAX_ALLOWED_DEPTH as usize];
        let mut depth = 0;
        loop {
            match first {
                Some(open @ (b'[' | b'{')) => {
                    if depth + 1 >= usize::from(self.remaining_depth) {
                        return perr!(self, RecursionLimitExceeded);
                    }
                    let close = if open.eq(&b'[') { b']' } else { b'}' };
                    if SKIP.not() {
                        if open.eq(&b'[') {
                            check_visit!(self, vis.visit_array_start(0))?;
                        } else {
                            check_visit!(self, vis.visit_object_start(0))?;
                        }
                    }
                    parents[depth] = (close, 0);
                    depth += 1;
                    first = self.skip_space();
                    if first.eq(&Some(close)) {
                        self.walk_end::<V, SKIP>(vis, close, 0)?;
                        depth -= 1;
                    } else {
                        if open.eq(&b'{') {
                            first = self.walk_key::<V, SKIP>(first, vis, strbuf.as_deref_mut())?;
                        }
                        continue;
                    }
                }
                Some(c @ (b'-' | b'0'..=b'9')) => {
                    if SKIP {
                        self.skip_number(c)?;
                    } else {
                        self.parse_number_visit(c, vis, strbuf.is_none())?;
                    }
                }
                Some(b'"') => {
                    if SKIP {
                        self.skip_string()?;
                    } else {
                        self.parse_string_visit(vis, strbuf.as_deref_mut())?;
                    }
                }
                Some(c) if SKIP => match c {
                    b't' => self.parse_literal("rue")?,
                    b'f' => self.parse_literal("alse")?,
                    b'n' => self.parse_literal("ull")?,
                    _ => return perr!(self, InvalidJsonValue),
                },
                Some(c) => self.parse_literal_visit(c, vis)?,
                None => return perr!(self, EofWhileParsing),
            }
            loop {
                if depth == 0 {
                    return Ok(());
                }
                if SKIP.not() {
                    parents[depth - 1].1 += 1;
                }
                let (close, count) = parents[depth - 1];
                match self.skip_space() {
                    Some(c) if c.eq(&close) => {
                        self.walk_end::<V, SKIP>(vis, close, count)?;
                        depth -= 1;
                    }
                    Some(b',') => {
                        first = self.skip_space();
                        if close.eq(&b'}') {
                            first = self.walk_key::<V, SKIP>(first, vis, strbuf.as_deref_mut())?;
                        }
                        break;
                    }
                    None => return perr!(self, EofWhileParsing),
                    _ if close.eq(&b']') => return perr!(self, ExpectedArrayCommaOrEnd),
                    _ => return perr!(self, ExpectedObjectCommaOrEnd),
                }
            }
        }
    }

    fn walk_end<V: JsonVisitor<'de>, const SKIP: bool>(
        &self,
        vis: &mut V,
        close: u8,
        count: usize,
    ) -> Result<()> {
        if SKIP.not() {
            if close.eq(&b']') {
                check_visit!(self, vis.visit_array_end(count))?;
            } else {
                check_visit!(self, vis.visit_object_end(count))?;
            }
        }
        Ok(())
    }

    fn walk_key<V: JsonVisitor<'de>, const SKIP: bool>(
        &mut self,
        first: Option<u8>,
        vis: &mut V,
        strbuf: Option<&mut Vec<u8>>,
    ) -> Result<Option<u8>> {
        match first {
            Some(b'"') => {}
            None => return perr!(self, EofWhileParsing),
            _ => return perr!(self, ExpectObjectKeyOrEnd),
        }
        if SKIP {
            self.skip_string()?;
        } else if strbuf.is_none() {
            self.parse_key_scalar(vis)?;
        } else {
            self.parse_string_visit(vis, strbuf)?;
        }
        self.parse_object_clo()?;
        Ok(self.skip_space())
    }
}
