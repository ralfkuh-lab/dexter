import "./App.css";
import { AgentDraft } from "./agent-draft/AgentDraft";
import { useAutomationConsoleLogging } from "./automation/console";
import { Orb } from "./orb/Orb";
import { Panel } from "./panel/Panel";
import { Settings } from "./settings/Settings";

function App() {
  useAutomationConsoleLogging();

  const params = new URLSearchParams(window.location.search);
  const view = params.get("view");

  if (view === "settings") {
    return <Settings />;
  }
  if (view === "panel") {
    return <Panel />;
  }
  if (view === "agent-draft") {
    return <AgentDraft />;
  }
  return <Orb />;
}

export default App;
