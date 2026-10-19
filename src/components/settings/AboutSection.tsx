import { useCallback, useEffect, useState } from "react";
import {
  BookOpen,
  Download,
  ExternalLink,
  Github,
  Globe,
  Info,
  LifeBuoy,
  Loader2,
  RefreshCw,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { useTranslation } from "react-i18next";
import { toast } from "@/lib/toast";
import { getVersion } from "@tauri-apps/api/app";
import { settingsApi } from "@/lib/api";
import { getProductInfo } from "@/lib/api/aimyria";
import { useUpdate } from "@/contexts/UpdateContext";
import { Badge } from "@/components/ui/badge";
import { AimyriaMark } from "@/components/brand/AimyriaWordmark";
import { extractErrorMessage } from "@/utils/errorUtils";
import type { ProductInfo } from "@/lib/types";

interface AboutSectionProps {
  isPortable: boolean;
}

// 应用自身版本（getVersion，本地毫秒级、无网络）也缓存一份，纯为重挂时免去 loading 闪烁。
let appVersionCache: string | null = null;

// 「源代码」按钮的目标。
//
// 它原本指向 `https://github.com/aimyria/aimyria-one`，**实测 404** —— 那个仓库
// 不存在。一个指向 404 的按钮比没有按钮更糟：用户以为项目在 GitHub 上，点进去
// 只看到 GitHub 的 404 页。既然公开仓库并不存在，就指向我们自己的站点里**真实
// 可达**的产品页。
//
// `/tutorial` 实测 **200**（GET 与 HEAD 一致）。`/docs/one-click` 与 `/support`
// 都落到 API 网关（**401**），所以那两个地址也不能用 —— 见
// `commands/aimyria.rs` 里 `product_info` 的同类修正。
const SOURCE_URL = "https://aimyria.com/tutorial";

/**
 * 设置 → 关于：版本、便携模式、检查更新 / 安装并重启、产品链接与许可声明。
 *
 * The links and the licence line come from the backend (`product_info`) rather
 * than from this file: this is a derivative work, and the MIT notice has to
 * travel with the binary. Keeping the strings in one place means About cannot
 * quietly drift from what the package actually is — an earlier version of this
 * screen still pointed at the upstream project's site and release notes.
 */
export function AboutSection({ isPortable }: AboutSectionProps) {
  const { t } = useTranslation();
  const [version, setVersion] = useState<string | null>(() => appVersionCache);
  const [isLoadingVersion, setIsLoadingVersion] = useState(
    () => appVersionCache === null,
  );
  const [isDownloading, setIsDownloading] = useState(false);
  const [info, setInfo] = useState<ProductInfo | null>(null);

  const { hasUpdate, updateInfo, checkUpdate, resetDismiss, isChecking } =
    useUpdate();

  useEffect(() => {
    let active = true;
    const loadAppVersion = async () => {
      try {
        const appVersion = await getVersion();
        appVersionCache = appVersion;
        if (active) {
          setVersion(appVersion);
        }
      } catch (error) {
        console.error("[AboutSection] Failed to load app version", error);
        if (active) {
          setVersion(null);
        }
      } finally {
        if (active) {
          setIsLoadingVersion(false);
        }
      }
    };

    void loadAppVersion();
    return () => {
      active = false;
    };
  }, []);

  useEffect(() => {
    let active = true;
    getProductInfo()
      .then((value) => {
        if (active) setInfo(value);
      })
      .catch((error) => {
        console.error("[AboutSection] Failed to load product info", error);
      });
    return () => {
      active = false;
    };
  }, []);

  const open = useCallback((url: string | undefined) => {
    if (url) void settingsApi.openExternal(url);
  }, []);

  const handleCheckUpdate = useCallback(async () => {
    if (hasUpdate) {
      if (isPortable) {
        try {
          await settingsApi.checkUpdates();
        } catch (error) {
          console.error("[AboutSection] Portable update failed", error);
        }
        return;
      }

      setIsDownloading(true);
      try {
        resetDismiss();
        const installed = await settingsApi.installUpdateAndRestart();
        if (!installed) {
          toast.success(t("settings.upToDate"), { closeButton: true });
        }
      } catch (error) {
        console.error("[AboutSection] Update failed", error);
        toast.error(t("settings.updateFailed"), {
          description: extractErrorMessage(error) || undefined,
          closeButton: true,
        });
        try {
          await settingsApi.checkUpdates();
        } catch (fallbackError) {
          console.error(
            "[AboutSection] Failed to open fallback updater",
            fallbackError,
          );
        }
      } finally {
        setIsDownloading(false);
      }
      return;
    }

    try {
      const available = await checkUpdate();
      if (!available) {
        toast.success(t("settings.upToDate"), { closeButton: true });
      }
    } catch (error) {
      console.error("[AboutSection] Check update failed", error);
      toast.error(t("settings.checkUpdateFailed"), {
        description: extractErrorMessage(error) || undefined,
        closeButton: true,
      });
    }
  }, [checkUpdate, hasUpdate, isPortable, resetDismiss, t]);

  const displayVersion = version ?? t("common.unknown");

  return (
    <div className="divide-y divide-border overflow-hidden rounded-panel border border-border bg-surface">
      <div className="flex flex-wrap items-center gap-4 px-5 py-4">
        <AimyriaMark size={40} className="shrink-0" />
        <div className="min-w-0 flex-1">
          <div className="text-title text-fg-1">
            {info?.name ?? "AimyriA go"}
          </div>
          <div className="flex flex-wrap items-center gap-2 text-caption text-fg-2">
            {isLoadingVersion ? (
              <Loader2 className="h-3 w-3 animate-spin" />
            ) : (
              <span className="tabular-nums">{`${t("common.version")} v${displayVersion}`}</span>
            )}
            {isPortable && (
              <Badge
                variant="secondary"
                className="gap-1 px-1.5 py-0 text-badge"
              >
                <Info className="h-3 w-3" />
                {t("settings.portableMode")}
              </Badge>
            )}
          </div>
        </div>
        <Button
          type="button"
          variant={hasUpdate ? "solid" : "neutral"}
          size="regular"
          onClick={handleCheckUpdate}
          disabled={isChecking || isDownloading}
        >
          {isDownloading ? (
            <>
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              {t("settings.updating")}
            </>
          ) : hasUpdate ? (
            <>
              <Download className="h-3.5 w-3.5" />
              {t("settings.updateTo", {
                version: updateInfo?.availableVersion ?? "",
              })}
            </>
          ) : isChecking ? (
            <>
              <RefreshCw className="h-3.5 w-3.5 animate-spin" />
              {t("settings.checking")}
            </>
          ) : (
            <>
              <RefreshCw className="h-3.5 w-3.5" />
              {t("settings.checkForUpdates")}
            </>
          )}
        </Button>
      </div>

      {hasUpdate && updateInfo && (
        <div className="space-y-1 px-5 py-4 text-body">
          <p className="font-medium text-fg-1">
            {t("settings.updateAvailable", {
              version: updateInfo.availableVersion,
            })}
          </p>
          {updateInfo.notes && (
            <p className="line-clamp-3 text-fg-2">{updateInfo.notes}</p>
          )}
        </div>
      )}

      <div className="flex flex-wrap items-center gap-2 px-5 py-4">
        <Button
          type="button"
          variant="neutral"
          size="compact"
          onClick={() => open(info?.website ?? "https://aimyria.com")}
        >
          <Globe className="h-3.5 w-3.5" />
          {t("settings.officialWebsite")}
        </Button>
        <Button
          type="button"
          variant="neutral"
          size="compact"
          // Fallbacks match `product_info`'s live values. The old ones here
          // (`/docs/one-click`, `/support`) both answer 501 on the live site.
          onClick={() => open(info?.docs ?? "https://aimyria.com/tutorial")}
        >
          <BookOpen className="h-3.5 w-3.5" />
          {t("settings.docs")}
        </Button>
        <Button
          type="button"
          variant="neutral"
          size="compact"
          onClick={() => open(info?.support ?? "https://aimyria.com/")}
        >
          <LifeBuoy className="h-3.5 w-3.5" />
          {t("settings.support")}
        </Button>
        <Button
          type="button"
          variant="neutral"
          size="compact"
          onClick={() => open(SOURCE_URL)}
        >
          <Github className="h-3.5 w-3.5" />
          {t("settings.source")}
        </Button>
      </div>

      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 px-5 py-4 text-caption text-fg-2">
        <span>
          {info?.attribution ?? t("settings.attribution")}
        </span>
        <span className="text-fg-3">·</span>
        <span>{info?.license ?? t("settings.license")}</span>
        <span className="text-fg-3">·</span>
        <span className="inline-flex items-center gap-1">
          <ExternalLink className="h-3 w-3" />
          {info?.website ?? "aimyria.com"}
        </span>
      </div>
    </div>
  );
}
