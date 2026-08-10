/**
 * Signing in to Apocrypha, from Settings.
 *
 * This used to show a code and ask someone to check it matched the website.
 * That question could be answered honestly and still hand an attacker a token,
 * because the app collecting the token was not necessarily the app the person
 * was looking at. So there is no longer a code, and nothing to compare: the
 * browser delivers the answer straight back to this process, on a socket it
 * opened before opening the browser.
 *
 * Which leaves this component with less to do than it had. It opens a page,
 * waits, and says what happened.
 *
 * Polling lives here rather than in Rust so the window stays responsive and
 * cancelling is instant. What is being polled is a local socket, not the
 * service. The token never passes through this file: Rust stores it when the
 * exchange succeeds, and all that comes back is "granted".
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { api, type ApocryphaAccountView, type AuthorizationStartedView } from "../lib/api";
import { Chip, Spinner } from "./ui";

export function ApocryphaAccount({
  onError,
  onInfo,
}: {
  onError: (e: unknown) => void;
  onInfo: (msg: string, kind?: "ok" | "bad" | "info") => void;
}) {
  const [account, setAccount] = useState<ApocryphaAccountView | null>(null);
  const [signingIn, setSigningIn] = useState<AuthorizationStartedView | null>(null);
  const [busy, setBusy] = useState(false);
  /**
   * Which sign-in attempt is the current one.
   *
   * A counter rather than a boolean, because a boolean cannot answer the
   * question that matters. Cancelling used to set a flag and immediately clear
   * it so the next attempt could run — which left the previous waiter alive,
   * polling the *new* attempt and eventually cancelling it when the old
   * deadline passed. Each waiter now remembers the number it was started with
   * and stops the moment it is not the current one.
   */
  const attempt = useRef(0);

  useEffect(() => {
    api.apocryphaAccount().then(setAccount).catch(onError);
    return () => {
      attempt.current += 1;
      // Leaving the window with a sign-in open would leave a socket listening
      // for an answer nobody is waiting for.
      void api.cancelApocryphaAuthorization().catch(() => {});
    };
  }, [onError]);

  /** Abandons whatever is in flight, and closes its socket. */
  const abandon = useCallback(() => {
    attempt.current += 1;
    setSigningIn(null);
    void api.cancelApocryphaAuthorization().catch(() => {});
  }, []);

  const waitFor = useCallback(
    async (started: AuthorizationStartedView, mine: number) => {
      const deadline = Date.now() + started.expiresInSeconds * 1000;
      const wait = started.pollIntervalSeconds * 1000;
      const current = () => attempt.current === mine;

      while (current() && Date.now() < deadline) {
        await new Promise((r) => setTimeout(r, wait));
        if (!current()) return;
        try {
          const result = await api.pollApocryphaAuthorization();
          if (!current()) return;
          if (result.status === "granted") {
            setSigningIn(null);
            setAccount(await api.apocryphaAccount());
            onInfo("Signed in to Apocrypha.", "ok");
            return;
          }
          if (result.status === "declined") {
            setSigningIn(null);
            onInfo("The request was declined. Nothing was granted.", "bad");
            return;
          }
        } catch (e) {
          if (!current()) return;
          setSigningIn(null);
          onError(e);
          return;
        }
      }
      if (current()) {
        abandon();
        onInfo("That sign-in expired. Try again.", "bad");
      }
    },
    [abandon, onError, onInfo],
  );

  const signIn = async () => {
    setBusy(true);
    // Claims this attempt before anything is opened, so any earlier waiter
    // stops on its next tick rather than polling this one.
    const mine = ++attempt.current;
    try {
      const started = await api.startApocryphaAuthorization();
      setSigningIn(started);
      try {
        await api.openUrl(started.authorizeUrl);
      } catch (e) {
        // The socket is already open and nothing is going to answer on it, so
        // it is closed here rather than left listening until the app exits.
        abandon();
        throw e;
      }
      void waitFor(started, mine);
    } catch (e) {
      onError(e);
    } finally {
      setBusy(false);
    }
  };

  const signOut = async () => {
    setBusy(true);
    try {
      setAccount(await api.signOutApocrypha());
      onInfo("Signed out on this computer.", "ok");
    } catch (e) {
      onError(e);
    } finally {
      setBusy(false);
    }
  };

  if (signingIn) {
    return (
      <div className="lib-group">
        <div className="pair-panel">
          <div className="pair-eyebrow">Waiting for your browser</div>
          <div className="pair-hint">
            Apocrypha has opened in your browser. Sign in there and allow this
            computer — the page will send you straight back.
          </div>
          <div className="row" style={{ gap: "var(--sp-2)" }}>
            <button
              className="btn sm"
              onClick={() => void api.openUrl(signingIn.authorizeUrl)}
            >
              Open the page again
            </button>
            <button className="btn sm" onClick={abandon}>
              Cancel
            </button>
            <span className="row" style={{ gap: "var(--sp-2)", marginLeft: "auto" }}>
              <Spinner />
              <span className="mod-meta">Waiting</span>
            </span>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="lib-group">
      <div className="lib-row">
        <span className="lib-row-label">Apocrypha account</span>
        <span className="lib-row-value">
          {account === null ? (
            "…"
          ) : account.signedIn ? (
            <span className="row" style={{ gap: "var(--sp-3)", justifyContent: "flex-end" }}>
              <span className="mono">{account.deviceName}</span>
              <Chip kind="ok">signed in</Chip>
            </span>
          ) : (
            "not signed in"
          )}
        </span>
        <span className="row" style={{ gap: "var(--sp-2)" }}>
          {account?.signedIn ? (
            <button className="btn sm" disabled={busy} onClick={() => void signOut()}>
              Sign out
            </button>
          ) : (
            <button className="btn primary sm" disabled={busy} onClick={() => void signIn()}>
              {busy ? <Spinner /> : null} Sign in
            </button>
          )}
        </span>
      </div>
      {account?.signedIn ? (
        <div className="lib-row">
          <span className="lib-row-label">Sign out</span>
          {/* Saying "revoke" would be a lie: the grant still exists on the
              service until it is revoked there. */}
          <span className="lib-row-value">
            Forgets the token on this computer only. Remove the device on the
            website to end the grant everywhere.
          </span>
        </div>
      ) : null}
    </div>
  );
}
