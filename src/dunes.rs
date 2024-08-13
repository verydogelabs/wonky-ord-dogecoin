use {
  self::{flag::Flag, tag::Tag},
  super::*,
};

pub use {edict::Edict, dune::Dune, dune_id::DuneId, dunestone::Dunestone, terms::Terms};

pub(crate) use {etching::Etching, pile::Pile, spaced_dune::SpacedDune};

pub(crate) const CLAIM_BIT: u128 = 1 << 48;
pub const MAX_DIVISIBILITY: u8 = 38;
pub(crate) const MAX_LIMIT: u128 = u64::MAX as u128;
const RESERVED: u128 = 6402364363415443603228541259936211926;

mod edict;
mod etching;
mod flag;
mod terms;
mod pile;
mod dune;
mod dune_id;
mod dunestone;
mod spaced_dune;
mod tag;
pub mod varint;

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, PartialEq)]
pub enum MintError {
  Cap(u128),
  End(u64),
  Start(u64),
  Unmintable,
}

impl Display for MintError {
  fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
    match self {
      MintError::Cap(cap) => write!(f, "limited to {cap} mints"),
      MintError::End(end) => write!(f, "mint ended on block {end}"),
      MintError::Start(start) => write!(f, "mint starts on block {start}"),
      MintError::Unmintable => write!(f, "not mintable"),
    }
  }
}

#[cfg(test)]
mod tests {
  use {super::*, crate::index::testing::Context};

  const DUNE: u128 = 99246114928149462;

  #[test]
  fn index_starts_with_no_dunes() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();
    context.assert_dunes([], []);
  }

  #[test]
  fn default_index_does_not_index_dunes() {
    let context = Context::builder().build();

    context.mine_blocks(1);

    context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes([], []);
  }

  #[test]
  fn empty_dunestone_does_not_create_dune() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(Dunestone::default().encipher()),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes([], []);
  }

  #[test]
  fn etching_with_no_edicts_creates_dune() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [],
    );
  }

  #[test]
  fn etching_with_edict_creates_dune() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(OutPoint { txid, vout: 0 }, vec![(id, u128::max_value())])],
    );
  }

  #[test]
  fn dunes_must_be_greater_than_or_equal_to_minimum_for_height() {
    {
      let context = Context::builder()
          .arg("--index-dunes")
          .build();

      context.mine_blocks(1);

      context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0, Witness::new())],
        op_return: Some(
          Dunestone {
            edicts: vec![Edict {
              id: 0,
              amount: u128::max_value(),
              output: 0,
            }],
            etching: Some(Etching {
              dune: Dune(DUNE - 1),
              ..Default::default()
            }),
            ..Default::default()
          }
              .encipher(),
        ),
        ..Default::default()
      });

      context.mine_blocks(1);

      context.assert_dunes([], []);
    }

    {
      let context = Context::builder()
          .arg("--index-dunes")
          .build();

      context.mine_blocks(1);

      let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
        inputs: &[(1, 0, 0, Witness::new())],
        op_return: Some(
          Dunestone {
            edicts: vec![Edict {
              id: 0,
              amount: u128::max_value(),
              output: 0,
            }],
            etching: Some(Etching {
              dune: Dune(DUNE),
              ..Default::default()
            }),
            ..Default::default()
          }
              .encipher(),
        ),
        ..Default::default()
      });

      context.mine_blocks(1);

      let id = DuneId {
        height: 2,
        index: 1,
      };

      context.assert_dunes(
        [(
          id,
          DuneEntry {
            etching: txid,
            dune: Dune(DUNE),
            supply: u128::max_value(),
            timestamp: 2,
            ..Default::default()
          },
        )],
        [(OutPoint { txid, vout: 0 }, vec![(id, u128::max_value())])],
      );
    }
  }

  #[test]
  fn etching_with_non_zero_divisibility_and_dune() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            divisibility: 1,
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          dune: Dune(DUNE),
          etching: txid,
          divisibility: 1,
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(OutPoint { txid, vout: 0 }, vec![(id, u128::max_value())])],
    );
  }

  #[test]
  fn allocations_over_max_supply_are_ignored() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: 0,
              amount: u128::max_value(),
              output: 0,
            },
            Edict {
              id: 0,
              amount: u128::max_value(),
              output: 0,
            },
          ],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(OutPoint { txid, vout: 0 }, vec![(id, u128::max_value())])],
    );
  }

  #[test]
  fn allocations_partially_over_max_supply_are_honored() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: 0,
              amount: u128::max_value() / 2,
              output: 0,
            },
            Edict {
              id: 0,
              amount: u128::max_value(),
              output: 0,
            },
          ],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          symbol: None,
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(OutPoint { txid, vout: 0 }, vec![(id, u128::max_value())])],
    );
  }

  #[test]
  fn etching_may_allocate_less_than_max_supply() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: 100,
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          supply: 100,
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(OutPoint { txid, vout: 0 }, vec![(id, 100)])],
    );
  }

  #[test]
  fn etching_may_allocate_to_multiple_outputs() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: 0,
              amount: 100,
              output: 0,
            },
            Edict {
              id: 0,
              amount: 100,
              output: 1,
            },
          ],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          burned: 100,
          etching: txid,
          dune: Dune(DUNE),
          supply: 200,
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(OutPoint { txid, vout: 0 }, vec![(id, 100)])],
    );
  }

  #[test]
  fn allocations_to_invalid_outputs_are_ignored() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: 0,
              amount: 100,
              output: 0,
            },
            Edict {
              id: 0,
              amount: 100,
              output: 3,
            },
          ],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          supply: 100,
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(OutPoint { txid, vout: 0 }, vec![(id, 100)])],
    );
  }

  #[test]
  fn input_dunes_may_be_allocated() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id, u128::max_value())],
      )],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: id.into(),
            amount: u128::max_value(),
            output: 0,
          }],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid1,
          vout: 0,
        },
        vec![(id, u128::max_value())],
      )],
    );
  }

  #[test]
  fn etched_dune_is_burned_if_an_unrecognized_even_tag_is_encountered() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          burn: true,
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [],
    );
  }

  #[test]
  fn input_dunes_are_burned_if_an_unrecognized_even_tag_is_encountered() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id, u128::max_value())],
      )],
    );

    context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          burn: true,
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          burned: u128::max_value(),
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [],
    );
  }

  #[test]
  fn unallocated_dunes_are_assigned_to_first_non_op_return_output() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id, u128::max_value())],
      )],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new())],
      op_return: Some(Dunestone::default().encipher()),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid1,
          vout: 0,
        },
        vec![(id, u128::max_value())],
      )],
    );
  }

  #[test]
  fn unallocated_dunes_in_transactions_with_no_dunestone_are_assigned_to_first_non_op_return_output(
  ) {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id, u128::max_value())],
      )],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new())],
      op_return: None,
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid1,
          vout: 0,
        },
        vec![(id, u128::max_value())],
      )],
    );
  }

  #[test]
  fn duplicate_dunes_are_forbidden() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(OutPoint { txid, vout: 0 }, vec![(id, u128::max_value())])],
    );

    context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(OutPoint { txid, vout: 0 }, vec![(id, u128::max_value())])],
    );
  }

  #[test]
  fn outpoint_may_hold_multiple_dunes() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id0 = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id0,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id0, u128::max_value())],
      )],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE + 1),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id1 = DuneId {
      height: 3,
      index: 1,
    };

    context.assert_dunes(
      [
        (
          id0,
          DuneEntry {
            etching: txid0,
            dune: Dune(DUNE),
            supply: u128::max_value(),
            timestamp: 2,
            ..Default::default()
          },
        ),
        (
          id1,
          DuneEntry {
            etching: txid1,
            dune: Dune(DUNE + 1),
            supply: u128::max_value(),
            timestamp: 3,
            number: 1,
            ..Default::default()
          },
        ),
      ],
      [
        (
          OutPoint {
            txid: txid0,
            vout: 0,
          },
          vec![(id0, u128::max_value())],
        ),
        (
          OutPoint {
            txid: txid1,
            vout: 0,
          },
          vec![(id1, u128::max_value())],
        ),
      ],
    );

    let txid2 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new()), (3, 1, 0, Witness::new())],
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [
        (
          id0,
          DuneEntry {
            etching: txid0,
            dune: Dune(DUNE),
            supply: u128::max_value(),
            timestamp: 2,
            ..Default::default()
          },
        ),
        (
          id1,
          DuneEntry {
            etching: txid1,
            dune: Dune(DUNE + 1),
            supply: u128::max_value(),
            timestamp: 3,
            number: 1,
            ..Default::default()
          },
        ),
      ],
      [(
        OutPoint {
          txid: txid2,
          vout: 0,
        },
        vec![(id0, u128::max_value()), (id1, u128::max_value())],
      )],
    );
  }

  #[test]
  fn multiple_input_dunes_on_the_same_input_may_be_allocated() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id0 = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id0,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id0, u128::max_value())],
      )],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE + 1),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id1 = DuneId {
      height: 3,
      index: 1,
    };

    context.assert_dunes(
      [
        (
          id0,
          DuneEntry {
            etching: txid0,
            dune: Dune(DUNE),
            supply: u128::max_value(),
            timestamp: 2,
            ..Default::default()
          },
        ),
        (
          id1,
          DuneEntry {
            etching: txid1,
            dune: Dune(DUNE + 1),
            supply: u128::max_value(),
            timestamp: 3,
            number: 1,
            ..Default::default()
          },
        ),
      ],
      [
        (
          OutPoint {
            txid: txid0,
            vout: 0,
          },
          vec![(id0, u128::max_value())],
        ),
        (
          OutPoint {
            txid: txid1,
            vout: 0,
          },
          vec![(id1, u128::max_value())],
        ),
      ],
    );

    let txid2 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new()), (3, 1, 0, Witness::new())],
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [
        (
          id0,
          DuneEntry {
            etching: txid0,
            dune: Dune(DUNE),
            supply: u128::max_value(),
            timestamp: 2,
            ..Default::default()
          },
        ),
        (
          id1,
          DuneEntry {
            etching: txid1,
            dune: Dune(DUNE + 1),
            supply: u128::max_value(),
            timestamp: 3,
            number: 1,
            ..Default::default()
          },
        ),
      ],
      [(
        OutPoint {
          txid: txid2,
          vout: 0,
        },
        vec![(id0, u128::max_value()), (id1, u128::max_value())],
      )],
    );

    let txid3 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(4, 1, 0, Witness::new())],
      outputs: 2,
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: id0.into(),
              amount: u128::max_value() / 2,
              output: 1,
            },
            Edict {
              id: id1.into(),
              amount: u128::max_value() / 2,
              output: 1,
            },
          ],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [
        (
          id0,
          DuneEntry {
            etching: txid0,
            dune: Dune(DUNE),
            supply: u128::max_value(),
            timestamp: 2,
            ..Default::default()
          },
        ),
        (
          id1,
          DuneEntry {
            etching: txid1,
            dune: Dune(DUNE + 1),
            supply: u128::max_value(),
            timestamp: 3,
            number: 1,
            ..Default::default()
          },
        ),
      ],
      [
        (
          OutPoint {
            txid: txid3,
            vout: 0,
          },
          vec![
            (id0, u128::max_value() / 2 + 1),
            (id1, u128::max_value() / 2 + 1),
          ],
        ),
        (
          OutPoint {
            txid: txid3,
            vout: 1,
          },
          vec![(id0, u128::max_value() / 2), (id1, u128::max_value() / 2)],
        ),
      ],
    );
  }

  #[test]
  fn multiple_input_dunes_on_different_inputs_may_be_allocated() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id0 = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id0,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id0, u128::max_value())],
      )],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE + 1),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id1 = DuneId {
      height: 3,
      index: 1,
    };

    context.assert_dunes(
      [
        (
          id0,
          DuneEntry {
            etching: txid0,
            dune: Dune(DUNE),
            supply: u128::max_value(),
            timestamp: 2,
            ..Default::default()
          },
        ),
        (
          id1,
          DuneEntry {
            etching: txid1,
            dune: Dune(DUNE + 1),
            supply: u128::max_value(),
            timestamp: 3,
            number: 1,
            ..Default::default()
          },
        ),
      ],
      [
        (
          OutPoint {
            txid: txid0,
            vout: 0,
          },
          vec![(id0, u128::max_value())],
        ),
        (
          OutPoint {
            txid: txid1,
            vout: 0,
          },
          vec![(id1, u128::max_value())],
        ),
      ],
    );

    let txid2 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new()), (3, 1, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: id0.into(),
              amount: u128::max_value(),
              output: 0,
            },
            Edict {
              id: id1.into(),
              amount: u128::max_value(),
              output: 0,
            },
          ],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [
        (
          id0,
          DuneEntry {
            etching: txid0,
            dune: Dune(DUNE),
            supply: u128::max_value(),
            timestamp: 2,
            ..Default::default()
          },
        ),
        (
          id1,
          DuneEntry {
            etching: txid1,
            dune: Dune(DUNE + 1),
            supply: u128::max_value(),
            timestamp: 3,
            number: 1,
            ..Default::default()
          },
        ),
      ],
      [(
        OutPoint {
          txid: txid2,
          vout: 0,
        },
        vec![(id0, u128::max_value()), (id1, u128::max_value())],
      )],
    );
  }

  #[test]
  fn unallocated_dunes_are_assigned_to_first_non_op_return_output_when_op_return_is_not_last_output(
  ) {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id, u128::max_value())],
      )],
    );

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new())],
      op_return: Some(
        script::Builder::new()
            .push_opcode(opcodes::all::OP_RETURN)
            .into_script(),
      ),
      op_return_index: Some(0),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(OutPoint { txid, vout: 1 }, vec![(id, u128::max_value())])],
    );
  }

  #[test]
  fn dune_rarity_is_assigned_correctly() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(2);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    let id0 = DuneId {
      height: 3,
      index: 1,
    };

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE + 1),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id1 = DuneId {
      height: 3,
      index: 2,
    };

    context.assert_dunes(
      [
        (
          id0,
          DuneEntry {
            etching: txid0,
            dune: Dune(DUNE),
            supply: u128::max_value(),
            timestamp: 3,
            ..Default::default()
          },
        ),
        (
          id1,
          DuneEntry {
            etching: txid1,
            dune: Dune(DUNE + 1),
            supply: u128::max_value(),
            timestamp: 3,
            number: 1,
            ..Default::default()
          },
        ),
      ],
      [
        (
          OutPoint {
            txid: txid0,
            vout: 0,
          },
          vec![(id0, u128::max_value())],
        ),
        (
          OutPoint {
            txid: txid1,
            vout: 0,
          },
          vec![(id1, u128::max_value())],
        ),
      ],
    );
  }

  #[test]
  fn edicts_with_id_zero_are_skipped() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id, u128::max_value())],
      )],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: 0,
              amount: 100,
              output: 0,
            },
            Edict {
              id: id.into(),
              amount: u128::max_value(),
              output: 0,
            },
          ],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid1,
          vout: 0,
        },
        vec![(id, u128::max_value())],
      )],
    );
  }

  #[test]
  fn edicts_which_refer_to_input_dune_with_no_balance_are_skipped() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id0 = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id0,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id0, u128::max_value())],
      )],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE + 1),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id1 = DuneId {
      height: 3,
      index: 1,
    };

    context.assert_dunes(
      [
        (
          id0,
          DuneEntry {
            etching: txid0,
            dune: Dune(DUNE),
            supply: u128::max_value(),
            timestamp: 2,
            ..Default::default()
          },
        ),
        (
          id1,
          DuneEntry {
            etching: txid1,
            dune: Dune(DUNE + 1),
            supply: u128::max_value(),
            timestamp: 3,
            number: 1,
            ..Default::default()
          },
        ),
      ],
      [
        (
          OutPoint {
            txid: txid0,
            vout: 0,
          },
          vec![(id0, u128::max_value())],
        ),
        (
          OutPoint {
            txid: txid1,
            vout: 0,
          },
          vec![(id1, u128::max_value())],
        ),
      ],
    );

    let txid2 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: id0.into(),
              amount: u128::max_value(),
              output: 0,
            },
            Edict {
              id: id1.into(),
              amount: u128::max_value(),
              output: 0,
            },
          ],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [
        (
          id0,
          DuneEntry {
            etching: txid0,
            dune: Dune(DUNE),
            supply: u128::max_value(),
            timestamp: 2,
            ..Default::default()
          },
        ),
        (
          id1,
          DuneEntry {
            etching: txid1,
            dune: Dune(DUNE + 1),
            supply: u128::max_value(),
            timestamp: 3,
            number: 1,
            ..Default::default()
          },
        ),
      ],
      [
        (
          OutPoint {
            txid: txid1,
            vout: 0,
          },
          vec![(id1, u128::max_value())],
        ),
        (
          OutPoint {
            txid: txid2,
            vout: 0,
          },
          vec![(id0, u128::max_value())],
        ),
      ],
    );
  }

  #[test]
  fn edicts_over_max_inputs_are_ignored() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value() / 2,
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value() / 2,
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id, u128::max_value() / 2)],
      )],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: id.into(),
            amount: u128::max_value(),
            output: 0,
          }],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value() / 2,
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid1,
          vout: 0,
        },
        vec![(id, u128::max_value() / 2)],
      )],
    );
  }

  #[test]
  fn edicts_may_transfer_dunes_to_op_return_outputs() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 1,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          burned: u128::max_value(),
          etching: txid,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [],
    );
  }

  #[test]
  fn outputs_with_no_dunes_have_no_balance() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      outputs: 2,
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(OutPoint { txid, vout: 0 }, vec![(id, u128::max_value())])],
    );
  }

  #[test]
  fn edicts_which_transfer_no_dunes_to_output_create_no_balance_entry() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      outputs: 2,
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: 0,
              amount: u128::max_value(),
              output: 0,
            },
            Edict {
              id: 0,
              amount: 0,
              output: 1,
            },
          ],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(OutPoint { txid, vout: 0 }, vec![(id, u128::max_value())])],
    );
  }

  #[test]
  fn split_in_etching() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      outputs: 4,
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: 0,
            output: 5,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          supply: (u128::max_value() / 4) * 4,
          timestamp: 2,
          ..Default::default()
        },
      )],
      [
        (
          OutPoint { txid, vout: 0 },
          vec![(id, u128::max_value() / 4)],
        ),
        (
          OutPoint { txid, vout: 1 },
          vec![(id, u128::max_value() / 4)],
        ),
        (
          OutPoint { txid, vout: 2 },
          vec![(id, u128::max_value() / 4)],
        ),
        (
          OutPoint { txid, vout: 3 },
          vec![(id, u128::max_value() / 4)],
        ),
      ],
    );
  }

  #[test]
  fn split_in_etching_with_preceding_edict() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      outputs: 4,
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: 0,
              amount: 1000,
              output: 0,
            },
            Edict {
              id: 0,
              amount: 0,
              output: 5,
            },
          ],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          supply: 1000 + ((u128::max_value() - 1000) / 4) * 4,
          timestamp: 2,
          ..Default::default()
        },
      )],
      [
        (
          OutPoint { txid, vout: 0 },
          vec![(id, 1000 + (u128::max_value() - 1000) / 4)],
        ),
        (
          OutPoint { txid, vout: 1 },
          vec![(id, (u128::max_value() - 1000) / 4)],
        ),
        (
          OutPoint { txid, vout: 2 },
          vec![(id, (u128::max_value() - 1000) / 4)],
        ),
        (
          OutPoint { txid, vout: 3 },
          vec![(id, (u128::max_value() - 1000) / 4)],
        ),
      ],
    );
  }

  #[test]
  fn split_in_etching_with_following_edict() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      outputs: 4,
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: 0,
              amount: 0,
              output: 5,
            },
            Edict {
              id: 0,
              amount: 1000,
              output: 0,
            },
          ],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [
        (
          OutPoint { txid, vout: 0 },
          vec![(id, u128::max_value() / 4 + 3)],
        ),
        (
          OutPoint { txid, vout: 1 },
          vec![(id, u128::max_value() / 4)],
        ),
        (
          OutPoint { txid, vout: 2 },
          vec![(id, u128::max_value() / 4)],
        ),
        (
          OutPoint { txid, vout: 3 },
          vec![(id, u128::max_value() / 4)],
        ),
      ],
    );
  }

  #[test]
  fn split_with_amount_in_etching() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      outputs: 4,
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: 1000,
            output: 5,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          supply: 4000,
          timestamp: 2,
          ..Default::default()
        },
      )],
      [
        (OutPoint { txid, vout: 0 }, vec![(id, 1000)]),
        (OutPoint { txid, vout: 1 }, vec![(id, 1000)]),
        (OutPoint { txid, vout: 2 }, vec![(id, 1000)]),
        (OutPoint { txid, vout: 3 }, vec![(id, 1000)]),
      ],
    );
  }

  #[test]
  fn split_in_etching_with_amount_with_preceding_edict() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      outputs: 4,
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: 0,
              amount: u128::max_value() - 3000,
              output: 0,
            },
            Edict {
              id: 0,
              amount: 1000,
              output: 5,
            },
          ],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [
        (
          OutPoint { txid, vout: 0 },
          vec![(id, u128::max_value() - 2000)],
        ),
        (OutPoint { txid, vout: 1 }, vec![(id, 1000)]),
        (OutPoint { txid, vout: 2 }, vec![(id, 1000)]),
      ],
    );
  }

  #[test]
  fn split_in_etching_with_amount_with_following_edict() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      outputs: 4,
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: 0,
              amount: 1000,
              output: 5,
            },
            Edict {
              id: 0,
              amount: u128::max_value(),
              output: 0,
            },
          ],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [
        (
          OutPoint { txid, vout: 0 },
          vec![(id, u128::max_value() - 4000 + 1000)],
        ),
        (OutPoint { txid, vout: 1 }, vec![(id, 1000)]),
        (OutPoint { txid, vout: 2 }, vec![(id, 1000)]),
        (OutPoint { txid, vout: 3 }, vec![(id, 1000)]),
      ],
    );
  }

  #[test]
  fn split() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id, u128::max_value())],
      )],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new())],
      outputs: 2,
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: id.into(),
            amount: 0,
            output: 3,
          }],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [
        (
          OutPoint {
            txid: txid1,
            vout: 0,
          },
          vec![(id, u128::max_value() / 2 + 1)],
        ),
        (
          OutPoint {
            txid: txid1,
            vout: 1,
          },
          vec![(id, u128::max_value() / 2)],
        ),
      ],
    );
  }

  #[test]
  fn split_with_preceding_edict() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id, u128::max_value())],
      )],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new())],
      outputs: 2,
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: id.into(),
              amount: 1000,
              output: 0,
            },
            Edict {
              id: id.into(),
              amount: 0,
              output: 3,
            },
          ],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [
        (
          OutPoint {
            txid: txid1,
            vout: 0,
          },
          vec![(id, 1000 + (u128::max_value() - 1000) / 2 + 1)],
        ),
        (
          OutPoint {
            txid: txid1,
            vout: 1,
          },
          vec![(id, (u128::max_value() - 1000) / 2)],
        ),
      ],
    );
  }

  #[test]
  fn split_with_following_edict() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id, u128::max_value())],
      )],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new())],
      outputs: 2,
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: id.into(),
              amount: 0,
              output: 3,
            },
            Edict {
              id: id.into(),
              amount: 1000,
              output: 1,
            },
          ],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [
        (
          OutPoint {
            txid: txid1,
            vout: 0,
          },
          vec![(id, u128::max_value() / 2)],
        ),
        (
          OutPoint {
            txid: txid1,
            vout: 1,
          },
          vec![(id, u128::max_value() / 2 + 1)],
        ),
      ],
    );
  }

  #[test]
  fn split_with_amount() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id, u128::max_value())],
      )],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new())],
      outputs: 2,
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: id.into(),
            amount: 1000,
            output: 3,
          }],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [
        (
          OutPoint {
            txid: txid1,
            vout: 0,
          },
          vec![(id, u128::max_value() - 1000)],
        ),
        (
          OutPoint {
            txid: txid1,
            vout: 1,
          },
          vec![(id, 1000)],
        ),
      ],
    );
  }

  #[test]
  fn split_with_amount_with_preceding_edict() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id, u128::max_value())],
      )],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new())],
      outputs: 4,
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: id.into(),
              amount: u128::max_value() - 2000,
              output: 0,
            },
            Edict {
              id: id.into(),
              amount: 1000,
              output: 5,
            },
          ],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [
        (
          OutPoint {
            txid: txid1,
            vout: 0,
          },
          vec![(id, u128::max_value() - 2000 + 1000)],
        ),
        (
          OutPoint {
            txid: txid1,
            vout: 1,
          },
          vec![(id, 1000)],
        ),
      ],
    );
  }

  #[test]
  fn split_with_amount_with_following_edict() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id, u128::max_value())],
      )],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new())],
      outputs: 4,
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: id.into(),
              amount: 1000,
              output: 5,
            },
            Edict {
              id: id.into(),
              amount: u128::max_value(),
              output: 0,
            },
          ],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [
        (
          OutPoint {
            txid: txid1,
            vout: 0,
          },
          vec![(id, u128::max_value() - 4000 + 1000)],
        ),
        (
          OutPoint {
            txid: txid1,
            vout: 1,
          },
          vec![(id, 1000)],
        ),
        (
          OutPoint {
            txid: txid1,
            vout: 2,
          },
          vec![(id, 1000)],
        ),
        (
          OutPoint {
            txid: txid1,
            vout: 3,
          },
          vec![(id, 1000)],
        ),
      ],
    );
  }

  #[test]
  fn etching_may_specify_symbol() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            symbol: Some('$'),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          symbol: Some('$'),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(OutPoint { txid, vout: 0 }, vec![(id, u128::max_value())])],
    );
  }

  #[test]
  fn allocate_all_remaining_dunes_in_etching() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: 0,
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(OutPoint { txid, vout: 0 }, vec![(id, u128::max_value())])],
    );
  }

  #[test]
  fn allocate_all_remaining_dunes_in_inputs() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: u128::max_value(),
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid0,
          vout: 0,
        },
        vec![(id, u128::max_value())],
      )],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 1, 0, Witness::new())],
      outputs: 2,
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: id.into(),
            amount: 0,
            output: 1,
          }],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          supply: u128::max_value(),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid1,
          vout: 1,
        },
        vec![(id, u128::max_value())],
      )],
    );
  }

  #[test]
  fn max_limit() {
    MAX_LIMIT
        .checked_mul(u128::from(u16::max_value()) * 144 * 365 * 1_000_000_000)
        .unwrap();
  }

  #[test]
  fn etching_with_limit_can_be_minted() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          etching: Some(Etching {
            dune: Dune(DUNE),
            limit: Some(1000),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          limit: Some(1000),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: u128::from(id) | CLAIM_BIT,
            amount: 1000,
            output: 0,
          }],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          limit: Some(1000),
          supply: 1000,
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid1,
          vout: 0,
        },
        vec![(id, 1000)],
      )],
    );

    let txid2 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(3, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: u128::from(id) | CLAIM_BIT,
            amount: 1000,
            output: 0,
          }],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          limit: Some(1000),
          supply: 2000,
          timestamp: 2,
          ..Default::default()
        },
      )],
      [
        (
          OutPoint {
            txid: txid2,
            vout: 0,
          },
          vec![(id, 1000)],
        ),
        (
          OutPoint {
            txid: txid1,
            vout: 0,
          },
          vec![(id, 1000)],
        ),
      ],
    );
  }

  #[test]
  fn open_etchings_can_be_limited_to_term() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          etching: Some(Etching {
            dune: Dune(DUNE),
            limit: Some(1000),
            term: Some(2),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          limit: Some(1000),
          end: Some(4),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: u128::from(id) | CLAIM_BIT,
            amount: 1000,
            output: 0,
          }],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          limit: Some(1000),
          supply: 1000,
          end: Some(4),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid1,
          vout: 0,
        },
        vec![(id, 1000)],
      )],
    );

    context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(3, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: u128::from(id) | CLAIM_BIT,
            amount: 1000,
            output: 0,
          }],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          limit: Some(1000),
          supply: 1000,
          end: Some(4),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: txid1,
          vout: 0,
        },
        vec![(id, 1000)],
      )],
    );
  }

  #[test]
  fn open_etchings_with_term_zero_cannot_be_minted() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: 0,
            amount: 1000,
            output: 0,
          }],
          etching: Some(Etching {
            dune: Dune(DUNE),
            limit: Some(1000),
            term: Some(0),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          limit: Some(1000),
          end: Some(2),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [],
    );

    context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 0, 0, Witness::new())],
      outputs: 2,
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: u128::from(id) | CLAIM_BIT,
            amount: 1,
            output: 3,
          }],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          limit: Some(1000),
          end: Some(2),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [],
    );
  }

  #[test]
  fn open_etching_claims_can_use_split() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid0 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          etching: Some(Etching {
            dune: Dune(DUNE),
            limit: Some(1000),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          limit: Some(1000),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [],
    );

    let txid1 = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 0, 0, Witness::new())],
      outputs: 2,
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: u128::from(id) | CLAIM_BIT,
            amount: 0,
            output: 3,
          }],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid0,
          dune: Dune(DUNE),
          limit: Some(1000),
          supply: 1000,
          timestamp: 2,
          ..Default::default()
        },
      )],
      [
        (
          OutPoint {
            txid: txid1,
            vout: 0,
          },
          vec![(id, 500)],
        ),
        (
          OutPoint {
            txid: txid1,
            vout: 1,
          },
          vec![(id, 500)],
        ),
      ],
    );
  }

  #[test]
  fn dunes_can_be_etched_and_claimed_in_the_same_transaction() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let txid = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          etching: Some(Etching {
            dune: Dune(DUNE),
            limit: Some(1000),
            ..Default::default()
          }),
          edicts: vec![Edict {
            id: 0,
            amount: 2000,
            output: 0,
          }],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching: txid,
          dune: Dune(DUNE),
          limit: Some(1000),
          timestamp: 2,
          supply: 1000,
          ..Default::default()
        },
      )],
      [(OutPoint { txid, vout: 0 }, vec![(id, 1000)])],
    );
  }

  #[test]
  fn limit_over_max_limit_is_ignored() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let etching = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          etching: Some(Etching {
            dune: Dune(DUNE),
            limit: Some(MAX_LIMIT + 1),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching,
          dune: Dune(DUNE),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [],
    );

    context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(2, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: u128::from(id) | CLAIM_BIT,
            amount: MAX_LIMIT + 1,
            output: 0,
          }],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching,
          dune: Dune(DUNE),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [],
    );
  }

  #[test]
  fn omitted_limit_defaults_to_max_limit() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let etching = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          etching: Some(Etching {
            dune: Dune(DUNE),
            term: Some(1),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching,
          dune: Dune(DUNE),
          limit: Some(MAX_LIMIT),
          end: Some(3),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [],
    );
  }

  #[test]
  fn transactions_cannot_claim_more_than_limit() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let etching = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          etching: Some(Etching {
            dune: Dune(DUNE),
            limit: Some(1000),
            ..Default::default()
          }),
          edicts: vec![Edict {
            id: 0,
            amount: 2000,
            output: 0,
          }],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching,
          dune: Dune(DUNE),
          limit: Some(1000),
          timestamp: 2,
          supply: 1000,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: etching,
          vout: 0,
        },
        vec![(id, 1000)],
      )],
    );

    let edict = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![Edict {
            id: u128::from(id) | CLAIM_BIT,
            amount: 2000,
            output: 0,
          }],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching,
          dune: Dune(DUNE),
          limit: Some(1000),
          timestamp: 2,
          supply: 2000,
          ..Default::default()
        },
      )],
      [
        (
          OutPoint {
            txid: etching,
            vout: 0,
          },
          vec![(id, 1000)],
        ),
        (
          OutPoint {
            txid: edict,
            vout: 0,
          },
          vec![(id, 1000)],
        ),
      ],
    );
  }

  #[test]
  fn multiple_edicts_in_one_transaction_may_claim_open_etching() {
    let context = Context::builder()
        .arg("--index-dunes")
        .build();

    context.mine_blocks(1);

    let etching = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          etching: Some(Etching {
            dune: Dune(DUNE),
            limit: Some(1000),
            ..Default::default()
          }),
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching,
          dune: Dune(DUNE),
          limit: Some(1000),
          timestamp: 2,
          ..Default::default()
        },
      )],
      [],
    );

    let edict = context.rpc_server.broadcast_tx(TransactionTemplate {
      inputs: &[(1, 0, 0, Witness::new())],
      op_return: Some(
        Dunestone {
          edicts: vec![
            Edict {
              id: u128::from(id) | CLAIM_BIT,
              amount: 500,
              output: 0,
            },
            Edict {
              id: u128::from(id) | CLAIM_BIT,
              amount: 500,
              output: 0,
            },
            Edict {
              id: u128::from(id) | CLAIM_BIT,
              amount: 500,
              output: 0,
            },
          ],
          ..Default::default()
        }
            .encipher(),
      ),
      ..Default::default()
    });

    context.mine_blocks(1);

    let id = DuneId {
      height: 2,
      index: 1,
    };

    context.assert_dunes(
      [(
        id,
        DuneEntry {
          etching,
          dune: Dune(DUNE),
          limit: Some(1000),
          timestamp: 2,
          supply: 1000,
          ..Default::default()
        },
      )],
      [(
        OutPoint {
          txid: edict,
          vout: 0,
        },
        vec![(id, 1000)],
      )],
    );
  }
}
