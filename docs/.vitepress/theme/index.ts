import DefaultTheme from "vitepress/theme";
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import "./custom.css";
import { installSidebarIcons } from "./sidebar-icons";

export default {
  extends: DefaultTheme,
  enhanceApp(ctx) {
    DefaultTheme.enhanceApp?.(ctx);
    installSidebarIcons();
  },
};
