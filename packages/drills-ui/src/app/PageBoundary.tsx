// Keeps a page's render error inside that page: the nav and the other pages keep working. Showing
// another route of the page (another run or test) tries again by itself.
import { Button } from "@primer/react";
import { Component, type ReactNode } from "react";
import { Notice } from "../components/Notice";

interface Props {
  label: string;
  resetKey: string;
  children: ReactNode;
}

export class PageBoundary extends Component<Props, { error: Error | null }> {
  state = { error: null as Error | null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidUpdate(previous: Props) {
    if (this.state.error && previous.resetKey !== this.props.resetKey) this.setState({ error: null });
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <main id="main" className="page-main">
        <Notice tone="error" title={`The ${this.props.label} page hit an error`} pre>
          {error.message}
        </Notice>
        <div>
          <Button size="small" onClick={() => this.setState({ error: null })}>Try again</Button>
        </div>
      </main>
    );
  }
}