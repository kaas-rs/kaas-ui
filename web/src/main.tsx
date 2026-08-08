import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import {
  Link,
  Outlet,
  RouterProvider,
  createRootRoute,
  createRoute,
  createRouter,
  redirect,
  useParams,
} from "@tanstack/react-router"

import "./styles.css"
import { AppLayout } from "@/layout"
import { PageTitle } from "@/components/page-title"
import { Empty } from "@/components/domain"
import { BASE_PATH } from "@/api/base"
import { TooltipProvider } from "@/components/ui/tooltip"
import { installTheme } from "@/lib/settings"
import { Fleet } from "@/pages/fleet"
import { Account } from "@/pages/account"
import { Settings } from "@/pages/settings"
import {
  CapabilitiesPage,
  ClusterConfigs,
  ClusterOverview,
} from "@/pages/cluster"
import { TopicDetail, Topics } from "@/pages/topics"
import { GroupDetail, Groups } from "@/pages/groups"
import { SchemaDetail, SchemaRegistry } from "@/pages/schema-registry"
import { EnvironmentPage } from "@/pages/environment"
import {
  messageSearchSchema,
  topicSearchSchema,
} from "@/features/messages/search"

const rootRoute = createRootRoute({ component: AppLayout })

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: Fleet,
})

/** Who this session is, and what it reaches. Not under a cluster: the answer
    spans all of them. */
const accountRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/account",
  component: Account,
})

/** What this browser does with kaas-ui, as opposed to what this session is.
    Not under a cluster either: none of it is a property of one. */
const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  component: Settings,
})

/**
 * Everything under an environment.
 *
 * The hierarchy the config has: a fleet holds environments, an environment
 * holds Kafka clusters and the schema registries beside them. The URL says so,
 * which also means a cluster id alone addresses nothing — `kafka` in `dev` and
 * `kafka` in `prod` are two clusters and both are reachable.
 */
const environmentRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/environments/$envId",
  component: Outlet,
})

/** The environment itself, at the bare `/environments/{env}`. */
const environmentIndexRoute = createRoute({
  getParentRoute: () => environmentRoute,
  path: "/",
  component: function EnvironmentIndex() {
    const { envId } = useParams({ from: "/environments/$envId" })
    return <EnvironmentPage envId={envId} />
  },
})

/** Everything under a cluster. Navigation lives in the sidebar, not in tabs. */
const clusterRoute = createRoute({
  getParentRoute: () => environmentRoute,
  path: "clusters/$clusterId",
  component: Outlet,
})

const overviewRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "/",
  component: function Overview() {
    const { envId, clusterId } = useParams({
      from: "/environments/$envId/clusters/$clusterId",
    })
    return <ClusterOverview envId={envId} clusterId={clusterId} />
  },
})

const topicsRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "topics",
  component: function TopicsPage() {
    const { envId, clusterId } = useParams({
      from: "/environments/$envId/clusters/$clusterId",
    })
    return <Topics envId={envId} clusterId={clusterId} />
  },
})

/**
 * The topic, and the message browser in a tab on it.
 *
 * The seek parameters are validated *here* because they are this page's state:
 * which tab, seeked where, filtered how, with which row selected. One URL
 * holds all of it, which is what makes a view someone is looking at a link
 * they can send.
 */
const topicRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "topics/$topic",
  validateSearch: topicSearchSchema,
  component: function TopicPage() {
    const { envId, clusterId, topic } = useParams({
      from: "/environments/$envId/clusters/$clusterId/topics/$topic",
    })
    return <TopicDetail envId={envId} clusterId={clusterId} topic={topic} />
  },
})

/**
 * Where the message browser used to live.
 *
 * It is a tab now, and this route stays only to keep the links people have
 * already sent each other working — validated and forwarded whole, so a URL
 * seeked to a timestamp with a row selected arrives on that exact view.
 */
const messagesRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "topics/$topic/messages",
  validateSearch: messageSearchSchema,
  beforeLoad: ({ params, search }) => {
    throw redirect({
      to: "/environments/$envId/clusters/$clusterId/topics/$topic",
      params,
      search: { ...search, tab: "messages" as const },
      replace: true,
    })
  },
})

const groupsRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "groups",
  component: function GroupsPage() {
    const { envId, clusterId } = useParams({
      from: "/environments/$envId/clusters/$clusterId",
    })
    return <Groups envId={envId} clusterId={clusterId} />
  },
})

const groupRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "groups/$groupId",
  component: function GroupPage() {
    const { envId, clusterId, groupId } = useParams({
      from: "/environments/$envId/clusters/$clusterId/groups/$groupId",
    })
    return <GroupDetail envId={envId} clusterId={clusterId} groupId={groupId} />
  },
})

const configsRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "configs",
  component: function ConfigsPage() {
    const { envId, clusterId } = useParams({
      from: "/environments/$envId/clusters/$clusterId",
    })
    return <ClusterConfigs envId={envId} clusterId={clusterId} />
  },
})

const capabilitiesRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "capabilities",
  component: function CapabilitiesRoute() {
    const { envId, clusterId } = useParams({
      from: "/environments/$envId/clusters/$clusterId",
    })
    return <CapabilitiesPage envId={envId} clusterId={clusterId} />
  },
})

/**
 * The schema browser.
 *
 * Under a cluster and not under a registry id: registry ids are deliberately
 * not a second enumerable namespace, and a caller reaches a registry only
 * through a cluster they can already see.
 */
const schemaRegistryRoute = createRoute({
  getParentRoute: () => environmentRoute,
  path: "schema-registries/$registryId",
  component: function SchemaRegistryPage() {
    const { envId, registryId } = useParams({
      from: "/environments/$envId/schema-registries/$registryId",
    })
    return <SchemaRegistry envId={envId} registryId={registryId} />
  },
})

/**
 * One subject.
 *
 * A subject name is a producer-chosen string and can hold a slash, so the
 * router's own escaping is what keeps `a/b-value` a single path parameter
 * rather than two segments.
 */
const schemaRoute = createRoute({
  getParentRoute: () => environmentRoute,
  path: "schema-registries/$registryId/subjects/$subject",
  component: function SchemaPage() {
    const { envId, registryId, subject } = useParams({
      from: "/environments/$envId/schema-registries/$registryId/subjects/$subject",
    })
    return (
      <SchemaDetail envId={envId} registryId={registryId} subject={subject} />
    )
  },
})

const routeTree = rootRoute.addChildren([
  indexRoute,
  accountRoute,
  settingsRoute,
  environmentRoute.addChildren([
    environmentIndexRoute,
    clusterRoute.addChildren([
      overviewRoute,
      topicsRoute,
      topicRoute,
      messagesRoute,
      groupsRoute,
      groupRoute,
      configsRoute,
      capabilitiesRoute,
    ]),
    schemaRegistryRoute,
    schemaRoute,
  ]),
])

/**
 * What an unknown path renders.
 *
 * Needed because the server serves `index.html` for anything that is not a
 * file — that fallback is what makes a hard refresh on a deep link work, and
 * the cost is that a genuinely wrong URL reaches the router rather than a 404
 * page. Without this it reaches the router's bare default, outside the app
 * chrome, which reads as a broken build rather than a mistyped link.
 */
function NotFound() {
  return (
    <div className="p-6">
      <PageTitle
        title="No such page"
        subtitle="The address does not match anything in this application."
      />
      <Empty>
        <Link to="/" className="underline">
          Back to the fleet
        </Link>
      </Empty>
    </div>
  )
}

const router = createRouter({
  routeTree,
  defaultPreload: "intent",
  defaultNotFoundComponent: NotFound,
  // Where the server said this page is mounted, read from the `<base>` it
  // injected. Without it the first client-side navigation leaves the prefix
  // and lands on the domain root — which looks like the app working until you
  // click something, and is the confusing half of a base-path problem.
  basepath: BASE_PATH || "/",
})

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router
  }
}

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // A cluster being unreachable is a fact to render, not a request to
      // retry three times before saying so.
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
})

// Keep the document's theme in step with the stored choice, from here on. The
// inline script in `index.html` already resolved it before first paint; this
// picks the same key up and keeps following the OS — outside React, because a
// listener that lives in a component stops at the first navigation away from it.
installTheme()

const container = document.getElementById("root")
if (container) {
  createRoot(container).render(
    <StrictMode>
      <QueryClientProvider client={queryClient}>
        <TooltipProvider delayDuration={200}>
          <RouterProvider router={router} />
        </TooltipProvider>
      </QueryClientProvider>
    </StrictMode>
  )
}
