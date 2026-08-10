/**
 * Signing in to Apocrypha, from Settings.
 *
 * The app never asks for a password. It shows a code and opens the website,
 * and a person approves it there — so the code is the whole interface while
 * pairing is in progress, set large and monospaced because the only thing to
 * do with it is compare it against the other screen.
 *
 * Polling lives here rather than in Rust so the window stays responsive and
 * cancelling is instant. The token never passes through this file: Rust stores
 * it when the service hands it over, and all that comes back is "granted".
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { api, type ApocryphaAccountView, type PairingStartedView } from "../lib/api";
import { Chip, Spinner } from "./ui";

export function ApocryphaAccount({
  onError,
  onInfo,
}: {
  onError: (e: unknown) => void;
  onInfo: (msg: string, kind?: "ok" | "bad" | "info") => void;
}) {
  const [account, setAccount] = useState<ApocryphaAccountView | null>(null);
  const [pairing, setPairing] = useState<PairingStartedView | null>(null);
  const [busy, setBusy] = useState(false);
  const cancelled = useRef(false);

  useEffect(() => {
    api.apocryphaAccount().then(setAccount).catch(onError);
    return () => {
      cancelled.current = true;
    };
  }, [onError]);

  const waitFor = useCallback(
    async (started: PairingStartedView) => {
      const deadline = Date.now() + started.expiresInSeconds * 1000;
      // The server sets the interval; a client that polls faster is told to
      // slow down rather than served, so backing off is the correct response
      // and not an error worth showing anyone.
      let wait = started.pollIntervalSeconds * 1000;

      while (!cancelled.current && Date.now() < deadline) {
        await new Promise((r) => setTimeout(r, wait));
        if (cancelled.current) return;
        try {
          const result = await api.pollApocryphaPairing(started.deviceCode);
          if (result.status === "granted") {
            setPairing(null);
            setAccount(await api.apocryphaAccount());
            onInfo("Signed in to Apocrypha.", "ok");
            return;
          }
          if (result.status === "slowDown") wait = Math.round(wait * 1.5);
        } catch (e) {
          setPairing(null);
          onError(e);
          return;
        }
      }
      if (!cancelled.current) {
        setPairing(null);
        onInfo("That code expired. Try again.", "bad");
      }
    },
    [onError, onInfo],
  );

  const signIn = async () => {
    setBusy(true);
    try {
      const started = await api.startApocryphaPairing();
      setPairing(started);
      await api.openUrl(started.approvalUrl);
      void waitFor(started);
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

  if (pairing) {
    return (
      <div className="lib-group">
        <div className="pair-panel">
          <div className="pair-eyebrow">Approve this code on the website</div>
          <div className="pair-code">{pairing.userCodeDisplay}</div>
          <div className="pair-hint">
            Your browser should have opened. Check the code there matches this
            one, then approve it.
          </div>
          <div className="row" style={{ gap: "var(--sp-2)" }}>
            <button className="btn sm" onClick={() => void api.openUrl(pairing.approvalUrl)}>
              Open the page again
            </button>
            <button
              className="btn sm"
              onClick={() => {
                cancelled.current = true;
                setPairing(null);
                // A new pairing gets a new code, so cancelling can be immediate
                // and the abandoned one simply expires.
                cancelled.current = false;
              }}
            >
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
