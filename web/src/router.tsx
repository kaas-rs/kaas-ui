// The route tree: URLs mirror the fleet's shape, exactly as the API does.

import {
  Link,
  Outlet,
  createRootRoute,
  createRoute,
  createRouter,
  redirect,
  useParams,
} from "@tanstack/react-router"
import type { SearchSchemaInput } from "@tanstack/react-router"
import { z } from "zod"

import { AppLayout } from "@/layout"
import { PageTitle } from "@/components/page-title"
import { Empty } from "@/components/domain"
import { BASE_PATH } from "@/api/base"
import { FleetPage } from "@/pages/fleet"
import { AccountPage } from "@/pages/account"
import { SettingsPage } from "@/pages/settings"
import { ClusterOverviewPage } from "@/pages/cluster-overview"
import { ClusterCapabilitiesPage } from "@/pages/cluster-capabilities"
import { ClusterConfigsPage } from "@/pages/cluster-configs"
import { ADMIN_TABS, ClusterAdminPage } from "@/pages/cluster-admin"
import type { AdminTab } from "@/pages/cluster-admin"
import { TopicsPage } from "@/pages/topics"
import { TopicDetailPage } from "@/pages/topic-detail"
import { GroupsPage } from "@/pages/groups"
import { GroupDetailPage } from "@/pages/group-detail"
import { SchemaRegistryPage } from "@/pages/schema-registry"
import { SchemaDetailPage } from "@/pages/schema-detail"
import { EnvironmentPage } from "@/pages/environment"
import {
  messageSearchSchema,
  topicSearchSchema,
} from "@/features/messages/search"

const rootRoute = createRootRoute({ component: AppLayout })

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: FleetPage,
})

/** Who this session is, and what it reaches. Not under a cluster: the answer
    spans all of them. */
const accountRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/account",
  component: AccountPage,
})

/** What this browser does with kaas-ui, as opposed to what this session is.
    Not under a cluster either: none of it is a property of one. */
const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  component: SettingsPage,
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
  component: function EnvironmentIndexRoute() {
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
  component: function OverviewRoute() {
    const { envId, clusterId } = useParams({
      from: "/environments/$envId/clusters/$clusterId",
    })
    return <ClusterOverviewPage envId={envId} clusterId={clusterId} />
  },
})

const topicsRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "topics",
  component: function TopicsRoute() {
    const { envId, clusterId } = useParams({
      from: "/environments/$envId/clusters/$clusterId",
    })
    return <TopicsPage envId={envId} clusterId={clusterId} />
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
  component: function TopicRoute() {
    const { envId, clusterId, topic } = useParams({
      from: "/environments/$envId/clusters/$clusterId/topics/$topic",
    })
    return <TopicDetailPage envId={envId} clusterId={clusterId} topic={topic} />
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
  component: function GroupsRoute() {
    const { envId, clusterId } = useParams({
      from: "/environments/$envId/clusters/$clusterId",
    })
    return <GroupsPage envId={envId} clusterId={clusterId} />
  },
})

const groupRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "groups/$groupId",
  component: function GroupRoute() {
    const { envId, clusterId, groupId } = useParams({
      from: "/environments/$envId/clusters/$clusterId/groups/$groupId",
    })
    return (
      <GroupDetailPage envId={envId} clusterId={clusterId} groupId={groupId} />
    )
  },
})

const configsRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "configs",
  component: function ConfigsRoute() {
    const { envId, clusterId } = useParams({
      from: "/environments/$envId/clusters/$clusterId",
    })
    return <ClusterConfigsPage envId={envId} clusterId={clusterId} />
  },
})

/**
 * The read-only admin surface: five screens behind one URL.
 *
 * The tab is a search parameter rather than a path segment, for the reason the
 * topic page's is: it is this page's state, and a link somebody sends should
 * open on the screen they were looking at. `.catch` on the enum means a
 * hand-edited or retired one lands on the ACLs rather than on an error
 * boundary — including `?screen=transactions` on a cluster that has none,
 * which is the URL the capability panel exists to answer.
 *
 * `screen` and not `tab`: the router types every search key across the whole
 * tree, so a second `tab` would widen the topic page's union with values it
 * has no tab for.
 */
const adminRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "admin",
  validateSearch: (input: { screen?: AdminTab } & SearchSchemaInput) =>
    ({ screen: z.enum(ADMIN_TABS).catch("acls").parse(input.screen) }) as {
      screen: AdminTab
    },
  component: function AdminRoute() {
    const { envId, clusterId } = useParams({
      from: "/environments/$envId/clusters/$clusterId",
    })
    return <ClusterAdminPage envId={envId} clusterId={clusterId} />
  },
})

const capabilitiesRoute = createRoute({
  getParentRoute: () => clusterRoute,
  path: "capabilities",
  component: function CapabilitiesRoute() {
    const { envId, clusterId } = useParams({
      from: "/environments/$envId/clusters/$clusterId",
    })
    return <ClusterCapabilitiesPage envId={envId} clusterId={clusterId} />
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
  component: function SchemaRegistryRoute() {
    const { envId, registryId } = useParams({
      from: "/environments/$envId/schema-registries/$registryId",
    })
    return <SchemaRegistryPage envId={envId} registryId={registryId} />
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
  component: function SchemaRoute() {
    const { envId, registryId, subject } = useParams({
      from: "/environments/$envId/schema-registries/$registryId/subjects/$subject",
    })
    return (
      <SchemaDetailPage
        envId={envId}
        registryId={registryId}
        subject={subject}
      />
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
      adminRoute,
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

export const router = createRouter({
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
