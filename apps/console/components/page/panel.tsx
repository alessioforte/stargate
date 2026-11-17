import classes from "./page.module.css";

interface Props {
  /** Controls on the right of the panel header. */
  actions?: React.ReactNode;
  children: React.ReactNode;
  /** Adds inner spacing; tables and editors span the panel edge to edge. */
  padded?: boolean;
  title?: React.ReactNode;
  /** Search and filters on the left of the panel header. */
  toolbar?: React.ReactNode;
}

const Panel: React.FC<Props> = ({
  actions,
  children,
  padded = false,
  title,
  toolbar,
}) => {
  const hasHeader = Boolean(title || toolbar || actions);

  return (
    <section className={classes.panel}>
      {hasHeader && (
        <div className={classes.panelHeader}>
          <div className={classes.panelStart}>
            {title && <h2 className={classes.panelTitle}>{title}</h2>}
            {toolbar}
          </div>
          {actions && <div className={classes.panelEnd}>{actions}</div>}
        </div>
      )}
      <div className={classes.panelBody} data-padded={padded || undefined}>
        {children}
      </div>
    </section>
  );
};

export default Panel;
