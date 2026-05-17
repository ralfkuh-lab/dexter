import "./App.css";
import { Orb } from "./orb/Orb";
import { Settings } from "./settings/Settings";

function App() {
  const params = new URLSearchParams(window.location.search);
  const view = params.get("view");

  if (view === "settings") {
    return <Settings />;
  }
  return <Orb />;
}

export default App;
