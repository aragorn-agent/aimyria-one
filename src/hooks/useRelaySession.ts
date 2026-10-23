/**
 * Relay account session — the desktop side of "one login, stays signed in".
 *
 * # Why the token never appears in this file
 *
 * The Rust backend owns the session: access token in memory, refresh credential
 * encrypted on disk. This hook only asks *whether* we are signed in and *who* we
 * are. That is what makes the session survive a page switch, a WebView reload, or
 * a window close-to-tray — there is nothing here to lose.
 *
 * # Why every call is refetch-based rather than cache-first
 *
 * The session can change from outside this window (sign-out on another device,
 * credential revoked from the web console). A stale "signed in" badge that lies is
 * worse than a brief loading state, so the status query refetches on focus and
 * after every mutation.
 */

import { useCallback, useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import {
  relayLogin,
  relayLogout,
  relayRefreshSession,
  relaySessionStatus,
  type RelayCommandError,
  type RelaySessionStatus,
} from "@/lib/api/relay";

export const RELAY_SESSION_KEY = ["relay-session"] as const;

export interface RelaySession {
  status: RelaySessionStatus | undefined;
  isLoading: boolean;
  isSignedIn: boolean;
  account: string;
  needsReauth: boolean;
  /** A session file exists but the server has not confirmed it yet. */
  unverified: boolean;
  signIn: (account: string, password: string) => Promise<void>;
  signOut: () => Promise<void>;
  refresh: () => Promise<void>;
}

export function useRelaySession(): RelaySession {
  const queryClient = useQueryClient();

  const query = useQuery({
    queryKey: RELAY_SESSION_KEY,
    queryFn: () => relaySessionStatus(),
    // Re-check when the window regains focus: a sign-out on another device should
    // be reflected without the user restarting the app.
    refetchOnWindowFocus: true,
    // The session is long-lived by design, so there is nothing to poll for.
    refetchInterval: false,
    staleTime: 30_000,
  });

  const unverified = query.data?.unverified ?? false;

  /**
   * A session off disk is unproven until the server honours it.
   *
   * This is the mechanism behind "never get logged out": on startup the app finds
   * a refresh credential, silently exchanges it, and is signed in before the user
   * sees anything. Doing it here rather than in a background timer means it only
   * happens when a screen actually needs the session.
   *
   * A failure is not surfaced as an error to the user: `relayRefreshSession`
   * clears a credential the server rejected, and the next status read shows the
   * login form. A network failure leaves everything intact and retries on the next
   * mount or focus.
   */
  useEffect(() => {
    if (!unverified) return;
    let cancelled = false;
    void (async () => {
      try {
        await relayRefreshSession();
      } catch {
        // Deliberately ignored — see above.
      }
      if (!cancelled) {
        // Everything derived from the session must be re-read, not just the
        // status: a screen that already mounted fetched its key list while
        // signed out, and would otherwise keep showing "登录账户查看全部密钥"
        // next to a session that is in fact live.
        await Promise.all([
          queryClient.invalidateQueries({ queryKey: RELAY_SESSION_KEY }),
          queryClient.invalidateQueries({ queryKey: ["relay-account-keys"] }),
          queryClient.invalidateQueries({ queryKey: ["relay-keys"] }),
        ]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [unverified, queryClient]);

  const signIn = useCallback(
    async (account: string, password: string) => {
      try {
        await relayLogin(account, password);
      } finally {
        // Invalidate even on failure: a partially-applied state is exactly the
        // case where the UI most needs the truth.
        await Promise.all([
          queryClient.invalidateQueries({ queryKey: RELAY_SESSION_KEY }),
          queryClient.invalidateQueries({ queryKey: ["relay-account-keys"] }),
        ]);
      }
    },
    [queryClient],
  );

  const signOut = useCallback(async () => {
    try {
      await relayLogout();
    } finally {
      await queryClient.invalidateQueries({ queryKey: RELAY_SESSION_KEY });
    }
  }, [queryClient]);

  const refresh = useCallback(async () => {
    try {
      const next = await relayRefreshSession();
      queryClient.setQueryData(RELAY_SESSION_KEY, next);
    } catch (error) {
      // A failed refresh means the caller should re-read the authoritative state
      // rather than trusting its own guess about what went wrong.
      await queryClient.invalidateQueries({ queryKey: RELAY_SESSION_KEY });
      throw error;
    }
  }, [queryClient]);

  return {
    status: query.data,
    isLoading: query.isLoading,
    isSignedIn: query.data?.signedIn ?? false,
    account: query.data?.account ?? "",
    needsReauth: query.data?.needsReauth ?? false,
    unverified,
    signIn,
    signOut,
    refresh,
  };
}

/** Narrow an unknown thrown value to the backend's `{ code, message }` shape. */
export function asRelayError(error: unknown): RelayCommandError {
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof (error as { message: unknown }).message === "string"
  ) {
    const candidate = error as Partial<RelayCommandError>;
    return {
      code: candidate.code ?? "unknown",
      message: candidate.message as string,
    };
  }
  return { code: "unknown", message: String(error) };
}
