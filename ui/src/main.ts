import { mount } from "svelte";
import App from "./App.svelte";
import "./styles/global.css";
import "./styles/hljs-dark.css";
import "./styles/chat-shared.css";

const app = mount(App, { target: document.getElementById("app")! });

export default app;
