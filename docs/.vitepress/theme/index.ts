import DefaultTheme from "vitepress/theme";
import "@fontsource-variable/geist";
import "./custom.css";
import { installSidebarIcons } from "./sidebar-icons";

export default {
  extends: DefaultTheme,
  enhanceApp(ctx) {
    DefaultTheme.enhanceApp?.(ctx);
    installSidebarIcons();
  },
};
