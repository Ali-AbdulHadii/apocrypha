/**
 * Deployment health check.
 *
 * Apocrypha's change log claims to know every file it put in the game folder,
 * but until now nothing ever checked that claim. Games patch, other mod tools
 * write files, and people delete things by hand. A manager that quietly trusts a
 * stale record is worse than one that admits it does not know, so this compares
 * the record against the disk and offers to put back what it can.
 *
 * Repair is deliberately opt-in per file. Putting back a file that something
 * else changed overwrites that change, and only the person using the app can
 * decide that is what they want.
 */

import { useState } from "react";
import { api, type FileVerdictView, type VerifyReportView } from "../lib/api";
import { Icon } from "./icons";
import { Chip, Spinner } from "./ui";

export function HealthCheck({
  gameId,
  onError,
  onInfo,
}: {
  gameId: string | null;
  onError: (e: unknown) => void;
  onInfo: (msg: string, kind?: "ok" | "bad" | "info") => void;
}) {
  const [report, setReport] = useState<VerifyReportView | null>(null);
  const [checking, setChecking] = useState(false);
  const [repairing, setRepairing] = useState(false);
  const [chosen, setChosen] = useState<Set<string>>(new Set());

  async function check() {
    if (!gameId) return;
    setChecking(true);
    try {
      const r = await api.verifyDeployment(gameId);
      setReport(r);
      // Missing files are the safe default: nothing of the user's is lost by
      // putting one back. A changed file is left unticked on purpose.
      setChosen(
        new Set(r.problems.filter((p) => p.state === "missing" && p.repairable).map((p) => p.path)),
      );
    } catch (e) {
      onError(e);
    } finally {
      setChecking(false);
    }
  }

  async function repair() {
    if (!gameId || chosen.size === 0) return;
    setRepairing(true);
    try {
      const r = await api.repairDeployment(gameId, [...chosen]);
      onInfo(
        r.repaired.length > 0
          ? `Put back ${r.repaired.length} file(s)`
          : "Nothing needed putting back",
        r.errors.length > 0 ? "bad" : "ok",
      );
      for (const problem of r.errors) onInfo(problem, "bad");
      await check();
    } catch (e) {
      onError(e);
    } finally {
      setRepairing(false);
    }
  }

  function toggle(path: string) {
    setChosen((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }

  const busy = checking || repairing;
  const repairable = report?.problems.filter((p) => p.repairable) ?? [];

  return (
    <div className="card stack">
      <div className="row">
        <div style={{ minWidth: 0, flex: 1 }}>
          <div className="card-title">Is everything still in place?</div>
          <div className="card-hint">
            Compares Apocrypha's record of what it wrote against the files in your
            game folder right now. Nothing is changed by looking.
          </div>
        </div>
        <button className="btn" onClick={check} disabled={busy || !gameId}>
          {checking ? <Spinner /> : <Icon.refresh size={14} />} Check now
        </button>
      </div>

      {report && (
        <>
          <div className="row" style={{ gap: "var(--sp-3)" }}>
            {report.intact ? (
              <Chip kind="ok">
                <span className="dot" /> All {report.checked} files match
              </Chip>
            ) : (
              <>
                <Chip kind="warn">
                  {report.problems.length} of {report.checked} need attention
                </Chip>
                <Chip>{report.ok} match</Chip>
              </>
            )}
          </div>

          {!report.intact && (
            <>
              <div className="file-list">
                {report.problems.map((p) => (
                  <ProblemRow
                    key={p.path}
                    problem={p}
                    checked={chosen.has(p.path)}
                    disabled={busy || !p.repairable}
                    onToggle={() => toggle(p.path)}
                  />
                ))}
              </div>

              <div className="row">
                <button
                  className="btn primary"
                  onClick={repair}
                  disabled={busy || chosen.size === 0}
                >
                  {repairing ? <Spinner /> : <Icon.undo size={14} />} Put back{" "}
                  {chosen.size} file{chosen.size === 1 ? "" : "s"}
                </button>
                <span className="card-hint">
                  {repairable.length < report.problems.length
                    ? "Files with no source left cannot be put back. Remove and add the mod again to fix those."
                    : "Changed files are left unticked, because putting one back discards whatever changed it."}
                </span>
              </div>
            </>
          )}
        </>
      )}
    </div>
  );
}

function ProblemRow({
  problem,
  checked,
  disabled,
  onToggle,
}: {
  problem: FileVerdictView;
  checked: boolean;
  disabled: boolean;
  onToggle: () => void;
}) {
  return (
    <label className="health-row">
      <input
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={onToggle}
      />
      <span className="mono truncate" title={problem.path}>
        {problem.path}
      </span>
      {problem.state === "missing" ? (
        <Chip kind="warn">gone</Chip>
      ) : (
        <Chip kind="warn">changed</Chip>
      )}
      {!problem.repairable && <Chip>cannot put back</Chip>}
    </label>
  );
}
