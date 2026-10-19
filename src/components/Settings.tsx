/**
 * Settings — the three things a user can change, and nothing else.
 */

import { openExternal } from "../lib/api";
import type { ProductInfo } from "../lib/types";
import { Row } from "./common";

export function Settings({
  autostart,
  autostartBusy,
  onToggleAutostart,
  info,
}: {
  autostart: boolean | null;
  autostartBusy: boolean;
  onToggleAutostart: (next: boolean) => void;
  info: ProductInfo | null;
}) {
  return (
    <>
      <h2 className="section-heading">通用</h2>
      <div className="rows">
        <Row
          title="开机自动启动"
          hint="登录 Windows 后在后台静默运行，不显示窗口"
        >
          <button
            className="toggle"
            role="switch"
            aria-checked={autostart === true}
            aria-label="开机自动启动"
            disabled={autostart === null || autostartBusy}
            onClick={() => onToggleAutostart(!autostart)}
          />
        </Row>
      </div>

      <h2 className="section-heading">帮助</h2>
      <div className="rows">
        <Row title="使用文档" hint="一键配置、计费与常见问题">
          <button
            className="button"
            onClick={() => info && void openExternal(info.docs)}
            disabled={!info}
          >
            打开
          </button>
        </Row>
        <Row title="联系支持" hint="配置没有生效时先看这里">
          <button
            className="button"
            onClick={() => info && void openExternal(info.support)}
            disabled={!info}
          >
            打开
          </button>
        </Row>
        <Row title="Aimyria 官网">
          <button
            className="button"
            onClick={() => info && void openExternal(info.website)}
            disabled={!info}
          >
            打开
          </button>
        </Row>
      </div>
    </>
  );
}