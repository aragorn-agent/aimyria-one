/**
 * About.
 *
 * The licence text lives in LICENSE, which ships with the binary. Nothing about
 * the derivative work is hidden — `aimyria.com` links there — but it is not
 * the first thing on screen either: this tool has one job, and the About page
 * is not where someone comes to read it.
 */

import { openExternal } from "../lib/api";
import type { ProductInfo } from "../lib/types";

export function About({ info }: { info: ProductInfo | null }) {
  return (
    <>
      <h2 className="about-title">{info?.name ?? "Aimyria 一键配置工具"}</h2>
      <div className="about-sub">版本 {info?.version ?? "—"}</div>

      <div className="prose">
        <p>这个工具只做一件事：把你的 AI 编程命令行指向 Aimyria。</p>
        <p>
          <button
            className="button button-quiet"
            onClick={() => info && void openExternal(info.website)}
          >
            {info?.website ?? "aimyria.com"}
          </button>
        </p>
      </div>

      <h2 className="section-heading">帮助</h2>
      <div className="prose">
        <p style={{ color: "var(--ink-3)", fontSize: 13 }}>
          使用文档、联系支持、以及本软件的许可与开源致谢，都可以在官网找到。
        </p>
      </div>
    </>
  );
}