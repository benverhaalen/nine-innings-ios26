use std::io::{self, Write};

// fwrite needs both the completed-byte count and the failure, unlike write_all.
// Interrupted attempts consume no bytes; retry without duplicating prior progress.
pub fn write_progress(writer: &mut impl Write, bytes: &[u8]) -> (usize, Option<io::Error>) {
    let mut done = 0;
    while done < bytes.len() {
        match writer.write(&bytes[done..]) {
            Ok(0) => return (done, Some(io::ErrorKind::WriteZero.into())),
            Ok(n) => done += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return (done, Some(e)),
        }
    }
    (done, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    struct FaultWriter { steps: VecDeque<io::Result<usize>>, output: Vec<u8> }
    impl Write for FaultWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let n = self.steps.pop_front().expect("unexpected extra write")?;
            assert!(n <= bytes.len());
            self.output.extend_from_slice(&bytes[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }
    #[test]
    fn short_and_interrupted_writes_preserve_exact_bytes() {
        let mut w = FaultWriter { steps: VecDeque::from([Ok(2), Err(io::ErrorKind::Interrupted.into()), Ok(1), Ok(3)]), output: vec![] };
        let (done, error) = write_progress(&mut w, b"abcdef");
        assert_eq!(done, 6); assert!(error.is_none()); assert_eq!(w.output, b"abcdef");
    }
    #[test]
    fn failure_preserves_partial_progress_and_original_error() {
        let mut w = FaultWriter { steps: VecDeque::from([Ok(3), Err(io::Error::from_raw_os_error(28))]), output: vec![] };
        let (done, error) = write_progress(&mut w, b"abcdef");
        assert_eq!(done, 3); assert_eq!(done / 2, 1); // one complete two-byte item
        assert_eq!(error.unwrap().raw_os_error(), Some(28)); assert_eq!(w.output, b"abc");
    }
    #[test]
    fn zero_progress_fails_without_looping_and_empty_write_is_noop() {
        let mut w = FaultWriter { steps: VecDeque::from([Ok(0)]), output: vec![] };
        let (done, error) = write_progress(&mut w, b"a");
        assert_eq!(done, 0); assert_eq!(error.unwrap().kind(), io::ErrorKind::WriteZero);
        let (done, error) = write_progress(&mut w, b"");
        assert_eq!(done, 0); assert!(error.is_none());
    }
}
