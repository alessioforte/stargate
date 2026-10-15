import { createBrowserRouter, RouterProvider } from "react-router";
import { routeObjects } from "./route-objects";
import { getAuthBasePath } from "@/lib/base-path";
import "./globals.css";

const router = createBrowserRouter(routeObjects, {
  basename: getAuthBasePath(),
});

function App() {
  return <RouterProvider router={router} />;
}

export default App;
