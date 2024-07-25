use super::*;

#[derive(Copy, Clone)]
#[repr(u8)]
pub(crate) enum Tag {
  #[allow(unused)]
  Parent = 3,
  #[allow(unused)]
  Metadata = 5,
  #[allow(unused)]
  Metaprotocol = 7,
  #[allow(unused)]
  ContentEncoding = 9,
  Delegate = 11,
  #[allow(unused)]
  Note = 15,
  #[allow(unused)]
  Nop = 255,
}

impl Tag {
  fn chunked(self) -> bool {
    matches!(self, Self::Metadata)
  }

  pub(crate) fn bytes(self) -> [u8; 1] {
    [self as u8]
  }

  pub(crate) fn take(self, fields: &mut BTreeMap<&[u8], Vec<&[u8]>>) -> Option<Vec<u8>> {
    if self.chunked() {
      let value = fields.remove(self.bytes().as_slice())?;

      if value.is_empty() {
        None
      } else {
        Some(value.into_iter().flatten().cloned().collect())
      }
    } else {
      let values = fields.get_mut(self.bytes().as_slice())?;

      if values.is_empty() {
        None
      } else {
        let value = values.remove(0).to_vec();

        if values.is_empty() {
          fields.remove(self.bytes().as_slice());
        }

        Some(value)
      }
    }
  }
}
