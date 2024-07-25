use {
  super::*,
  bitcoin::hashes::{sha256d},
};

pub(crate) struct Rtx<'a>(pub(crate) redb::ReadTransaction<'a>);

impl Rtx<'_> {
  pub(crate) fn height(&self) -> Result<Option<Height>> {
    Ok(
      self
        .0
        .open_table(HEIGHT_TO_BLOCK_HASH)?
        .range(0..)?
        .rev()
        .next()
        .map(|result| {
          result.map(|(height, _hash)| Height(height.value()))
        })
        .transpose()? // Converts Option<Result<T, E>> to Result<Option<T>, E>
    )
  }

  pub(crate) fn block_count(&self) -> Result<u32> {
    Ok(
      self
        .0
        .open_table(HEIGHT_TO_BLOCK_HASH)?
        .range(0..)?
        .rev()
        .next()
        .map(|result| {
          result.map(|(height, _hash)| height.value() + 1)
        })
        .transpose()?  // Converts Option<Result<T, E>> to Result<Option<T>, E> and propagates error if any
        .unwrap_or(0),
    )
  }


  pub(crate) fn block_hash(&self, height: Option<u32>) -> Result<Option<BlockHash>> {
    let height_to_block_header = self.0.open_table(HEIGHT_TO_BLOCK_HASH)?;

    Ok(
      match height {
        Some(height) => height_to_block_header.get(height)?
          .map(|header| {
            let block_hash_value = header.value().clone();
            let sha256d_hash = sha256d::Hash::from_slice(&block_hash_value)
              .expect("Invalid block hash");
            BlockHash::from(sha256d_hash)
          }),
        None => height_to_block_header
          .range(0..)?
          .next_back()
          .transpose()?
          .map(|(_height, header)| {
            let block_hash_value = header.value().clone();
            let sha256d_hash = sha256d::Hash::from_slice(&block_hash_value)
              .expect("Invalid block hash");
            BlockHash::from(sha256d_hash)
          }),
      }
    )
  }
}
