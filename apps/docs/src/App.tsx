import './App.css'

function App() {
  return (
    <div className="docs">
      <aside><b>Boson</b><span>Documentation</span><nav>Introduction<br/>Architecture<br/>Configuration<br/>Admin API<br/>Extensions<br/>Deployment</nav></aside>
      <article>
        <small>GETTING STARTED</small>
        <h1>A backend platform, not another framework.</h1>
        <p className="lead">Boson provides a modular Rust server, background worker, operational dashboard, developer CLI, and stable extension contracts.</p>
        <h2>Architecture</h2>
        <pre>{`Dashboard / CLI\n       ↓\n  Admin API\n       ↓\nCapabilities → Ports → Providers`}</pre>
        <h2>Run locally</h2>
        <pre>{`cargo run -p boson-server\ncargo run -p boson-cli -- doctor\nnpm run dev --prefix apps/dashboard`}</pre>
        <h2>Stable principles</h2>
        <ul>
          <li>Dashboard and CLI are clients of the Admin API.</li>
          <li>Capabilities own their data and communicate through contracts.</li>
          <li>Provider SDKs stay behind ports.</li>
          <li>Operational visibility is part of the platform.</li>
        </ul>
      </article>
    </div>
  )
}

export default App
