use super::*;

#[derive(Boilerplate, Debug, PartialEq, Serialize, Deserialize)]
pub struct DuneBalancesHtml {
  pub balances: BTreeMap<SpacedDune, BTreeMap<OutPoint, u128>>,
}

impl PageContent for DuneBalancesHtml {
  fn title(&self) -> String {
    "Dune Balances".to_string()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  const DUNE: u128 = 99246114928149462;

  #[test]
  fn display_dune_balances() {
    let balances: BTreeMap<Dune, BTreeMap<OutPoint, u128>> = vec![
      (
        Dune(DUNE),
        vec![(
          OutPoint {
            txid: txid(1),
            vout: 1,
          },
          1000,
        )]
        .into_iter()
        .collect(),
      ),
      (
        Dune(DUNE + 1),
        vec![(
          OutPoint {
            txid: txid(2),
            vout: 2,
          },
          12345678,
        )]
        .into_iter()
        .collect(),
      ),
    ]
    .into_iter()
    .collect();

    assert_regex_match!(
      DuneBalancesHtml { balances }.to_string(),
      "<h1>Dune Balances</h1>
<table>
  <tr>
    <th>dune</th>
    <th>balances</th>
  </tr>
  <tr>
    <td><a href=/dune/AAAAAAAAAAAAA>.*</a></td>
    <td>
      <table>
        <tr>
          <td class=monospace>
            <a href=/output/1{64}:1>1{64}:1</a>
          </td>
          <td class=monospace>
            1000
          </td>
        </tr>
      </table>
    </td>
  </tr>
  <tr>
    <td><a href=/dune/AAAAAAAAAAAAB>.*</a></td>
    <td>
      <table>
        <tr>
          <td class=monospace>
            <a href=/output/2{64}:2>2{64}:2</a>
          </td>
          <td class=monospace>
            12345678
          </td>
        </tr>
      </table>
    </td>
  </tr>
</table>
"
      .unindent()
    );
  }
}
