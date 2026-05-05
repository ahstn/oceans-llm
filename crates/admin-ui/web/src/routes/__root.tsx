/// <reference types="vite/client" />

import type { ReactNode } from "react";
import type { QueryClient } from "@tanstack/react-query";
import { createIsomorphicFn } from "@tanstack/react-start";
import {
  HeadContent,
  Navigate,
  Outlet,
  Scripts,
  createRootRouteWithContext,
  redirect,
  useRouterState,
} from "@tanstack/react-router";
import { Toaster } from "sonner";

import { AppShell } from "@/components/layout/app-shell";
import { GlobalErrorPage } from "@/components/layout/global-error-page";
import { TooltipProvider } from "@/components/ui/tooltip";
import { getAuthSession } from "@/server/admin-data.functions";
import globalsCss from "@/styles/globals.css?url";
import faviconUrl from "../../../../../docs/images/oceans-logo-rounded-square.png?url";
import {
  DEFAULT_SIGNED_IN_PATH,
  buildRedirectTarget,
  isPlatformAdminSession,
  isPublicAdminRoute,
  normalizeAdminPath,
} from "@/routes/-auth-routing";

const loadAuthSession = createIsomorphicFn()
  .server(async () => {
    const { getSession } = await import("@/server/admin-data.server");
    return getSession();
  })
  .client(() => getAuthSession());

export const Route = createRootRouteWithContext<{ queryClient: QueryClient }>()(
  {
    beforeLoad: async ({ location }) => {
      const currentPath = normalizeAdminPath(location.pathname);
      const isPublicRoute = isPublicAdminRoute(currentPath);
      const { data: session } = await loadAuthSession();
      const adminSession = isPlatformAdminSession(session) ? session : null;

      if (isPublicRoute) {
        if (currentPath === "/login" && adminSession) {
          throw redirect({
            to: adminSession.must_change_password
              ? "/change-password"
              : DEFAULT_SIGNED_IN_PATH,
          });
        }

        if (currentPath === "/change-password" && !adminSession) {
          throw redirect({
            to: "/login",
            search: { redirect: "/change-password" },
          });
        }

        return { session: adminSession };
      }

      if (!adminSession) {
        throw redirect({
          to: "/login",
          search: {
            redirect: buildRedirectTarget(location.pathname, location.search),
          },
        });
      }

      if (
        adminSession.must_change_password &&
        currentPath !== "/change-password"
      ) {
        throw redirect({ to: "/change-password" });
      }

      return { session: adminSession };
    },
    errorComponent: RootErrorComponent,
    head: () => ({
      meta: [
        { charSet: "utf-8" },
        { name: "viewport", content: "width=device-width, initial-scale=1" },
        { title: "Oceans Gateway Admin" },
        {
          name: "description",
          content: "Oceans LLM gateway control plane powered by TanStack Start",
        },
      ],
      links: [
        { rel: "stylesheet", href: globalsCss },
        { rel: "icon", type: "image/png", href: faviconUrl },
        { rel: "apple-touch-icon", href: faviconUrl },
      ],
    }),
    component: RootComponent,
  },
);

function RootErrorComponent(props: Parameters<typeof GlobalErrorPage>[0]) {
  return (
    <RootDocument>
      <GlobalErrorPage {...props} />
    </RootDocument>
  );
}

function RootComponent() {
  const pathname = useRouterState({
    select: (state) => state.location.pathname,
  });
  const currentPath = normalizeAdminPath(pathname);
  const isPublicRoute = isPublicAdminRoute(currentPath);
  const { session } = Route.useRouteContext();

  if (!isPublicRoute && session?.must_change_password) {
    return (
      <RootDocument>
        <Navigate to="/change-password" />
      </RootDocument>
    );
  }

  return (
    <RootDocument>
      {isPublicRoute ? (
        <Outlet />
      ) : session ? (
        <AppShell session={session}>
          <Outlet />
        </AppShell>
      ) : null}
    </RootDocument>
  );
}

function RootDocument({ children }: { children: ReactNode }) {
  return (
    <html lang="en" className="dark">
      <head>
        <HeadContent />
      </head>
      <body>
        <TooltipProvider>{children}</TooltipProvider>
        <Toaster
          position="top-right"
          theme="dark"
          toastOptions={{
            style: {
              background: "var(--card)",
              border: "1px solid var(--border)",
              color: "var(--foreground)",
            },
          }}
        />
        <Scripts />
      </body>
    </html>
  );
}
