import { render } from "solid-js/web";
import App from "./App";
import LiveApp from "./LiveApp";
import ScoreboardApp from "./ScoreboardApp";
import V2App from "./V2App";
import V2SimApp from "./V2SimApp";

const root = document.getElementById("app")!;
const mode = (window as any).__MODE__;

if (mode === "live") {
  render(() => <LiveApp />, root);
} else if (mode === "scoreboard") {
  render(() => <ScoreboardApp />, root);
} else if (mode === "v2rr") {
  render(() => <V2App />, root);
} else if (mode === "v2sim") {
  render(() => <V2SimApp />, root);
} else {
  render(() => <App />, root);
}
