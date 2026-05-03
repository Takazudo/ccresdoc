import DefaultLayout from "../layouts/default";

export default function NotFoundPage() {
  return (
    <DefaultLayout title="404 — Page not found | CCResDoc">
      <h1>404</h1>
      <p>The page you are looking for does not exist.</p>
      <p>
        <a href="/">Return to the home page</a>
      </p>
    </DefaultLayout>
  );
}
