import "./App.css";
import { Orb } from "./orb/Orb";
import { Panel } from "./panel/Panel";
import { Settings } from "./settings/Settings";

function App() {
  const params = new URLSearchParams(window.location.search);
  const view = params.get("view");

  if (view === "settings") {
    return <Settings />;
  }
  if (view === "panel") {
    return <Panel />;
  }
  return <Orb />;
}

export default App;
