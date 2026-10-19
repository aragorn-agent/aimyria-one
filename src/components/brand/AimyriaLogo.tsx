import { AimyriaMark, AimyriaWordmark } from "@/components/brand/AimyriaWordmark";

/**
 * Compact brand lockup used in page headers.
 *
 * `withWordmark` uses the official mark + vector wordmark; the bare form is
 * just the mark, for slots where the page title already spells the product.
 *
 * The mark carries its own tint (`text-brand-mark`: black on light, white on
 * dark) so it is not overridden here — passing `text-action` from a caller would
 * paint the logo near-white on a white surface.
 */
export function AimyriaLogo({
  size = 24,
  withWordmark = false,
  className,
}: {
  size?: number;
  className?: string;
  withWordmark?: boolean;
}) {
  if (withWordmark) {
    return <AimyriaWordmark size={size} className={className} />;
  }
  return <AimyriaMark size={size} className={className} />;
}
