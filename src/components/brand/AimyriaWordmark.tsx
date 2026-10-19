import type { CSSProperties } from "react";
import markUrl from "@/assets/brand/aimyria-mark.svg";
import wordmarkUrl from "@/assets/brand/aimyria-wordmark.svg";
import lockupUrl from "@/assets/brand/aimyria-lockup.svg";
import { cn } from "@/lib/utils";

// The owner-approved AIMYRIA vector pack (2026-10-17). These files are the
// traced originals, not a redraw — the previous component approximated the mark
// with three triangles and a gradient, which is exactly why it never looked like
// the brand. Using them as CSS masks keeps the silhouette exact at 16 px and at
// 512 px, and lets `currentColor` tint them for light/dark surfaces.
const MARK_ASPECT = 832.228 / 699.535;
const WORDMARK_ASPECT = 1172.202 / 217.017;
const LOCKUP_ASPECT = 1172.2 / 1013.3;

function maskStyle(url: string, extra?: CSSProperties): CSSProperties {
  return {
    WebkitMaskImage: `url(${url})`,
    maskImage: `url(${url})`,
    WebkitMaskRepeat: "no-repeat",
    maskRepeat: "no-repeat",
    WebkitMaskSize: "contain",
    maskSize: "contain",
    WebkitMaskPosition: "center",
    maskPosition: "center",
    ...extra,
  };
}

/** The triangle + north-star mark, tinted by the surrounding text colour. */
export function AimyriaMark({
  size = 20,
  className,
  style,
}: {
  /** Height in px; width follows the official aspect ratio. */
  size?: number;
  className?: string;
  style?: CSSProperties;
}) {  return (
    <span
      aria-hidden="true"
      className={cn("inline-block shrink-0 bg-current text-brand-mark", className)}
      style={{
        width: Math.round(size * MARK_ASPECT * 100) / 100,
        height: size,
        ...maskStyle(markUrl, style),
      }}
    />
  );
}

/**
 * Mark + official wordmark + the product suffix “go”.
 *
 * The wordmark is the real vector, not letterspaced UI text: the brand's
 * silhouette and spacing live in the path data, and retyping it in Montserrat
 * would be a different mark at every browser.
 */
export function AimyriaWordmark({
  size = 18,
  showTagline = false,
  className,
}: {
  /** Height of the mark in px. The wordmark scales to 0.68 of it. */
  size?: number;
  showTagline?: boolean;
  className?: string;
}) {
  const wordHeight = Math.max(10, size * 0.68);
  return (
    <span className={cn("flex min-w-0 items-center gap-1.5", className)}>
      <AimyriaMark size={size} />
      <span className="flex min-w-0 flex-col justify-center leading-none">
        <span className="flex items-baseline gap-1">
          <span
            aria-hidden="true"
            className="inline-block shrink-0 bg-current text-brand-mark"
            style={{
              width: Math.round(wordHeight * WORDMARK_ASPECT * 100) / 100,
              height: wordHeight,
              ...maskStyle(wordmarkUrl),
            }}
          />
          <span className="text-[0.62em] font-semibold lowercase tracking-[0.02em] text-brand-mark">
            go
          </span>
        </span>
        {showTagline ? (
          <span className="mt-1 truncate text-[0.55rem] tracking-[0.22em] text-fg-3 uppercase">
            私有中转 · 一键接入
          </span>
        ) : null}
      </span>
    </span>
  );
}

/** The full vertical lockup (mark above wordmark) for hero surfaces. */
export function AimyriaLockup({
  size = 120,
  className,
}: {
  /** Height in px; width follows the official aspect ratio. */
  size?: number;
  className?: string;
}) {
  return (
    <span
      aria-hidden="true"
      className={cn("inline-block bg-current", className)}
      style={{
        width: Math.round(size * LOCKUP_ASPECT * 100) / 100,
        height: size,
        ...maskStyle(lockupUrl),
      }}
    />
  );
}
