import { compareVersions } from "@/lib/version";

/**
 * 更新摘要：每个版本一份 `src/whats-new/<version>.json`，构建时打进安装包。
 * 新版安装包天然带着它自己和之前所有版本的摘要，弹窗不联网。格式见同目录 README。
 */

export const WHATS_NEW_ITEM_TYPES = ["new", "fix", "improve"] as const;
export type WhatsNewItemType = (typeof WHATS_NEW_ITEM_TYPES)[number];

export const WHATS_NEW_LANGUAGES = ["zh", "zh-TW", "en", "ja"] as const;
export type WhatsNewLanguage = (typeof WHATS_NEW_LANGUAGES)[number];

export type WhatsNewItem = { type: WhatsNewItemType } & Record<
  WhatsNewLanguage,
  string
>;

export interface WhatsNewEntry {
  version: string;
  /** 空数组 = 这一版不弹窗（纯构建、CI 之类的版本） */
  items: WhatsNewItem[];
}

const modules = import.meta.glob<WhatsNewEntry>("../whats-new/*.json", {
  eager: true,
  import: "default",
});

/** 所有版本的摘要，新到旧 */
export const WHATS_NEW_ENTRIES: WhatsNewEntry[] = Object.values(modules).sort(
  (a, b) => compareVersions(b.version, a.version),
);

/** 当前版本比记录的已看版本新（或从没记录过）：需要处理一次 */
export function isNewerThanSeen(
  current: string,
  seen: string | undefined,
): boolean {
  return !seen || compareVersions(current, seen) > 0;
}

/**
 * 启动弹窗要显示的版本：`(seen, current]` 里有内容的版本，新到旧。
 * 没有记录（从还没有这个功能的版本升上来）时不知道起点，只显示当前版本。
 */
export function entriesSince(
  entries: WhatsNewEntry[],
  seen: string | undefined,
  current: string,
): WhatsNewEntry[] {
  return entries.filter((entry) => {
    if (entry.items.length === 0) return false;
    if (!seen) return entry.version === current;
    return (
      compareVersions(entry.version, seen) > 0 &&
      compareVersions(entry.version, current) <= 0
    );
  });
}

/** 关于页手动查看：不超过当前版本、有内容的版本，新到旧 */
export function entriesUpTo(
  entries: WhatsNewEntry[],
  current: string,
): WhatsNewEntry[] {
  return entries.filter(
    (entry) =>
      entry.items.length > 0 && compareVersions(entry.version, current) <= 0,
  );
}

/**
 * 看过之后要记下的版本：只往高记。降级后不把记录改低，
 * 再升回来时看过的版本不会重复出现。
 */
export function markSeen(
  seen: string | undefined,
  current: string | undefined,
): string | undefined {
  if (!current) return seen;
  return seen && compareVersions(seen, current) > 0 ? seen : current;
}

export function resolveWhatsNewLanguage(language: string): WhatsNewLanguage {
  return (WHATS_NEW_LANGUAGES as readonly string[]).includes(language)
    ? (language as WhatsNewLanguage)
    : "en";
}

/**
 * 官网更新日志：不带版本是列表页。
 *
 * 这里原本是 `https://ccswitch.io/...` —— **上游 CC-Switch 的站点**。与
 * `commands/misc.rs` 里那个竞品 releases 链接是同一类问题：一句话就会把用户
 * 送去竞品。
 *
 * 改指我们自己的发布通道。**刻意不拼 `/${siteLanguage}/changelog`**：我们站上
 * 没有这个页面（`/docs`、`/download`、`/support` 以及任何未注册路径都落到 API
 * 网关，401），照搬上游的 URL 结构只会制造一串错误页。
 *
 * **这是一个过渡决定**：现在指向自更新清单 `version.json` 描述的那个载荷本身，
 * 实测 200 且带 `Content-Disposition: attachment`，点一下就是下载最新版。
 * 一旦 T5 在 `latest/` 下补出真正的下载页（`/releases/latest/` 目前是 403，
 * 因为 `autoindex off`），把这里换成那个页面即可 —— 更得体，也能放按版本的说明。
 */
export function changelogUrl(
  language: WhatsNewLanguage,
  version?: string,
): string {
  // 语言与版本号暂时不影响目标：站上还没有按语言/按版本分页的日志页。
  // 参数保留在签名里，是为了在页面就绪时不必改动任何调用点。
  void (language === "zh-TW" ? "zh" : language);
  void version;
  return "https://aimyria.com/releases/latest/download/AimyriaOne-x64.exe";
}
