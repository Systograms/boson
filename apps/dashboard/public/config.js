// Empty apiUrl selects same-origin Admin API when the Dashboard is served by
// the Server. Split deployments can set an absolute URL, for example:
// window.__BOSON_CONFIG__ = { apiUrl: "https://api.example.com" }
window.__BOSON_CONFIG__ = { apiUrl: "" }
