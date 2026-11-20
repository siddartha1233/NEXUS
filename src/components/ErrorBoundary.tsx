import React, { Component, type ReactNode } from "react";

interface Props {
  children: ReactNode;
  fallbackTitle?: string;
}

interface State {
  hasError: boolean;
  error?: Error;
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false };
  }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    console.error("UI Error caught by boundary:", error, errorInfo);
  }

  handleRetry = () => {
    this.setState({ hasError: false, error: undefined });
  };

  render() {
    if (this.state.hasError) {
      return (
        <div style={{ padding: "40px", maxWidth: "600px", margin: "auto" }}>
          <h2 style={{ color: "#f85149", marginBottom: "10px" }}>
            {this.props.fallbackTitle || "Something went wrong in this view"}
          </h2>
          <p style={{ color: "#888888", fontSize: "13px", marginBottom: "20px" }}>
            {this.state.error?.message || "An unexpected error occurred."}
          </p>
          <button
            onClick={this.handleRetry}
            style={{
              padding: "8px 16px",
              background: "#ffffff",
              color: "#000000",
              border: "none",
              borderRadius: "6px",
              fontWeight: 600,
              cursor: "pointer",
            }}
          >
            Retry View
          </button>
        </div>
      );
    }

    return this.props.children;
  }
}
