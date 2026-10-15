import { Navigate, useLocation } from "react-router";

export default function HomeRedirect() {
  const location = useLocation();

  return <Navigate to={`/login${location.search}`} replace />;
}
