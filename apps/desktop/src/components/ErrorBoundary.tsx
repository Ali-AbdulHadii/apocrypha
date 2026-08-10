/**
 * The thing that stops one bad value emptying the window.
 *
 * React unmounts the entire tree when a render throws and nothing catches it.
 * There was no boundary anywhere in this app, so any unhandled error — a field
 * that came back null, a list that was a string, one malformed response from an
 * API — replaced the whole interface with nothing at all. No message, no way
 * back, and nothing on screen to suggest what had happened or that the app was
 * still running.
 *
 * That is what "the UI of most pages disappeared" was. The trigger was a Nexus
 * response; the reason it took the application with it was the absence of this
 * file.
 *
 * It deliberately shows the error rather than a friendly apology. This is a
 * tool for people who mod games: the message and the component stack are the
 * two things that make a report useful, and hiding them helps nobody.
 */

import { Component, type ErrorInfo, type ReactNode } from "react";
import { supportMailto } from "../lib/support";
import { api } from "../lib/api";

interface Props {
  children: ReactNode;
  /** Named in the message, so a person can say which part broke. */
  area?: string;
}

interface State {
  error: Error | null;
  stack: string | null;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null, stack: null };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    // The console is where a developer looks first, and this is the only copy
    // of the component stack — React does not put it on the Error.
    console.error("Unhandled render error", error, info.componentStack);
    this.setState({ stack: info.componentStack ?? null });
  }

  private reset = () => this.setState({ error: null, stack: null });

  render() {
    const { error, stack } = this.state;
    if (!error) return this.props.children;

    const area = this.props.area ? ` in ${this.props.area}` : "";
    const report = `${error.message}\n\n${stack ?? ""}`.trim();

    return (
      <div className="crash">
        <div className="crash-card">
          <h2>Something broke{area}</h2>
          <p className="crash-lede">
            The rest of Apocrypha is still running. Nothing was written to your
            game folder by this — deployments only happen when you apply, and an
            interrupted apply rolls back on its own.
          </p>

          <pre className="crash-detail">{error.message}</pre>

          <div className="crash-actions">
            {/* Recovering in place rather than reloading. A reload loses
                whatever else was on screen, and this usually clears. */}
            <button className="btn primary" onClick={this.reset}>
              Try again
            </button>
            <button
              className="btn"
              onClick={() => void navigator.clipboard?.writeText(report)}
            >
              Copy details
            </button>
            <button
              className="btn"
              onClick={() => void api.openUrl(supportMailto("crash"))}
            >
              Report it
            </button>
          </div>

          {stack ? (
            <details className="crash-stack">
              <summary>Where it happened</summary>
              <pre>{stack}</pre>
            </details>
          ) : null}
        </div>
      </div>
    );
  }
}
