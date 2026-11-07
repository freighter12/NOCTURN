// Keeps one failing page from blanking the whole app: the sidebar, player and
// other pages keep working, and the user can retry or go Home.

import { recordError } from "../lib/errors";
import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  /** Changing this (e.g. the page ID) clears the error. */
  resetKey: string;
  onHome: () => void;
  children: ReactNode;
}

export class ErrorBoundary extends Component<Props, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("page crashed", error, info.componentStack);
    recordError(`Page crashed: ${error.message}`);
  }

  componentDidUpdate(prev: Props) {
    if (prev.resetKey !== this.props.resetKey && this.state.error) this.setState({ error: null });
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="page">
        <div className="panel" role="alert">
          <h2 style={{ marginTop: 0 }}>This page hit a problem</h2>
          <p className="muted">The rest of Nocturn is fine. Details: {this.state.error.message}</p>
          <div className="row" style={{ gap: 8 }}>
            <button className="btn" onClick={() => this.setState({ error: null })}>Try again</button>
            <button className="btn ghost" onClick={this.props.onHome}>Go to Home</button>
          </div>
        </div>
      </div>
    );
  }
}
