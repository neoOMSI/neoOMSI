import { useEffect, useState, type DependencyList } from "react";
import { parse, type Route } from "./routes";

export function useRoute(initial?: Route) {
  const [route, setRoute] = useState(
    () => initial ?? parse(location.pathname, location.hash),
  );
  useEffect(() => {
    const change = () => setRoute(parse(location.pathname, location.hash));
    window.addEventListener("navigate", change);
    window.addEventListener("popstate", change);
    return () => {
      window.removeEventListener("navigate", change);
      window.removeEventListener("popstate", change);
    };
  }, []);
  return route;
}

export function useTitle(title?: string) {
  useEffect(() => {
    if (title) document.title = `${title} | neoOMSI`;
  }, [title]);
}

export type Async<T> =
  | { data: T; error?: undefined }
  | { data?: undefined; error: Error }
  | { data?: undefined; error?: undefined };

export function useAsync<T>(
  load: () => Promise<T>,
  deps: DependencyList,
): Async<T> {
  const [state, setState] = useState<Async<T>>({});
  useEffect(() => {
    let live = true;
    setState({});
    load().then(
      (data) => live && setState({ data }),
      (error: Error) => live && setState({ error }),
    );
    return () => {
      live = false;
    };
  }, deps);
  return state;
}
