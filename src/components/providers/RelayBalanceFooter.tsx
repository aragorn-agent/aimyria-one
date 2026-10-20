/**
 * The balance line on the relay's own provider card.
 *
 * # Why this exists
 *
 * The relay row used to show nothing, because the card deliberately excluded it:
 * `usageEnabled` had a `!relayEndpoint` clause whose comment said the balance
 * belongs on the 接入中心 page. The owner's report was the cost of that choice --
 * a third-party relay (FlowNode) showed "余额 9.93 USD" while ours, on the same
 * screen, showed nothing at all, which reads as "our relay has no balance".
 *
 * The number itself was never missing: the read-only balance bridge is deployed
 * and `get_relay_balance` has been calling it all along. So this footer consumes
 * the same command the 接入中心 page uses, and the two now agree by construction
 * rather than by coincidence.
 *
 * # Why it never invents a number
 *
 * A failed lookup renders as "暂不可用" with the reason, never as `$0.00`. The two
 * facts are different -- "we could not ask" does not mean "you have no money" --
 * and only the second should send someone to top up. `BalanceChip` already draws
 * that distinction; this component keeps it.
 */
import { useQuery } from "@tanstack/react-query";
import { getRelayBalance } from "@/lib/api/aimyria";
import { BalanceChip } from "@/components/common";

/** The relay balance, refreshed on the same cadence as the 接入中心 page. */
const RELAY_BALANCE_QUERY_KEY = ["relay-balance"];

export default function RelayBalanceFooter({
  isCurrent,
}: {
  isCurrent: boolean;
}) {
  const { data, isLoading, isError, error } = useQuery({
    queryKey: RELAY_BALANCE_QUERY_KEY,
    queryFn: () => getRelayBalance(),
    // The 接入中心 page already polls this key every 60s. Reusing the key means
    // the card and that page share one request instead of racing two, so the
    // interval here only matters when nobody has that page open.
    refetchInterval: isCurrent ? 60_000 : false,
    staleTime: 30_000,
  });

  if (isLoading && !data) {
    return <BalanceChip remaining={null} low={false} error={null} loading />;
  }

  if (isError || !data) {
    // A transport failure and a "no bridge yet" are reported the same way the
    // page reports them: a plain reason, not a fabricated amount.
    const reason = error instanceof Error ? error.message : "暂时无法查询";
    return (
      <BalanceChip remaining={null} low={false} error={reason} loading={false} />
    );
  }

  // `reason` distinguishes "not imported yet" / "relay has no key-scoped balance
  // endpoint" / "the bridge exists but failed". The chip renders the last case as
  // 暂不可用; the first two are honest states of their own and read better named.
  if (data.remaining === null && data.reason) {
    const text =
      data.reason === "not_bound"
        ? "未导入密钥"
        : data.reason === "bridge_unavailable"
          ? "余额待开通"
          : (data.error ?? "暂不可用");
    return <BalanceChip remaining={null} low={false} error={text} loading={false} />;
  }

  return (
    <BalanceChip
      remaining={data.remaining}
      low={data.low}
      error={data.error}
      loading={false}
    />
  );
}
