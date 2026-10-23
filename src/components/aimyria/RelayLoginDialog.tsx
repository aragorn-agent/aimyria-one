/**
 * Sign-in dialog for the Aimyria console.
 *
 * # Design constraints this follows
 *
 * * **One step, one screen.** Account, password, submit. No tabs, no "forgot
 *   password" flow inside the app — that belongs on the website, and a link out
 *   is honest about it.
 * * **Never clears what the user typed on failure.** A rejected password should
 *   leave the field selected so they can fix a typo, not retype an email address.
 * * **The error says which failure it was.** "Wrong password" and "we are
 *   offline" are different problems; collapsing them trains users to reset a
 *   password that was never wrong.
 * * **No "remember me".** The session is long-lived by product decision; a
 *   checkbox would imply it might not be.
 */

import { useEffect, useRef, useState } from "react";
import { Loader2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { asRelayError, useRelaySession } from "@/hooks/useRelaySession";
import { relayErrorHint } from "@/lib/api/relay";
import { CONSOLE_URL, openExternal } from "@/lib/api/aimyria";

interface RelayLoginDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Shown above the form when the dialog was opened by an expired session. */
  reason?: "expired" | null;
}

export function RelayLoginDialog({
  open,
  onOpenChange,
  reason = null,
}: RelayLoginDialogProps) {
  const { signIn } = useRelaySession();
  const [account, setAccount] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ code: string; message: string } | null>(
    null,
  );
  const accountRef = useRef<HTMLInputElement>(null);
  const passwordRef = useRef<HTMLInputElement>(null);

  // Reset the transient parts when the dialog opens, but keep nothing across
  // opens: a password must not survive a close.
  useEffect(() => {
    if (!open) {
      setPassword("");
      setError(null);
      setBusy(false);
      return;
    }
    setError(null);
    // Focus after the dialog has mounted, otherwise Radix steals it back.
    const handle = window.setTimeout(() => {
      if (account.trim()) {
        passwordRef.current?.focus();
      } else {
        accountRef.current?.focus();
      }
    }, 60);
    return () => window.clearTimeout(handle);
  }, [open]);

  const submit = async () => {
    if (busy) return;
    if (!account.trim() || !password) {
      setError({ code: "invalid_input", message: "请输入账号和密码" });
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await signIn(account, password);
      setPassword("");
      onOpenChange(false);
    } catch (caught) {
      const relayError = asRelayError(caught);
      setError(relayError);
      // Keep the email, clear only the password: retyping an address is pure
      // friction, and keeping a wrong password invites a second blind attempt.
      setPassword("");
      window.setTimeout(() => passwordRef.current?.focus(), 30);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(next) => !busy && onOpenChange(next)}>
      <DialogContent className="max-w-[380px]">
        <DialogHeader className="space-y-2 border-b-0 bg-transparent pb-0">
          <DialogTitle className="text-base font-semibold">
            登录 Aimyria
          </DialogTitle>
          <DialogDescription className="text-sm leading-6">
            {reason === "expired"
              ? "会话已结束，重新登录后会恢复到原来的状态。已接入的客户端不受影响。"
              : "登录后可以在这里创建密钥、查看余额和用量，不用再打开网页。"}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-3 px-6 pt-4">
          <label className="block space-y-1.5">
            <span className="text-caption text-fg-2">账号</span>
            <Input
              ref={accountRef}
              value={account}
              disabled={busy}
              autoComplete="username"
              placeholder="邮箱"
              onChange={(event) => setAccount(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  passwordRef.current?.focus();
                }
              }}
            />
          </label>

          <label className="block space-y-1.5">
            <span className="text-caption text-fg-2">密码</span>
            <Input
              ref={passwordRef}
              type="password"
              value={password}
              disabled={busy}
              autoComplete="current-password"
              placeholder="密码"
              aria-invalid={error?.code === "rejected" ? true : undefined}
              onChange={(event) => setPassword(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void submit();
                }
              }}
            />
          </label>

          {error ? (
            <div className="space-y-1">
              <p className="text-caption text-danger-text">{error.message}</p>
              {relayErrorHint(error) ? (
                <p className="text-[11px] leading-5 text-fg-3">
                  {relayErrorHint(error)}
                </p>
              ) : null}
            </div>
          ) : null}
        </div>

        <DialogFooter className="flex items-center justify-between gap-2 border-t-0 bg-transparent pt-3 sm:justify-between">
          <button
            type="button"
            disabled={busy}
            onClick={() => void openExternal(CONSOLE_URL)}
            className="text-caption text-fg-3 transition-colors hover:text-fg-1 disabled:opacity-50"
          >
            在网页登录 ↗
          </button>
          <div className="flex gap-2">
            <Button
              variant="outline"
              disabled={busy}
              onClick={() => onOpenChange(false)}
            >
              取消
            </Button>
            <Button disabled={busy} onClick={() => void submit()}>
              {busy ? (
                <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
              ) : null}
              登录
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
