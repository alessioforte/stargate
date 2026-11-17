import classes from "./page.module.css";

interface Props {
  /** Buttons aligned with the title. */
  actions?: React.ReactNode;
  /** Badges shown next to the title. */
  badges?: React.ReactNode;
  children: React.ReactNode;
  description?: React.ReactNode;
  title: React.ReactNode;
}

const Page: React.FC<Props> = ({
  actions,
  badges,
  children,
  description,
  title,
}) => {
  return (
    <div className={classes.page}>
      <div className={classes.header}>
        <div className={classes.heading}>
          <div className={classes.titleRow}>
            <h1 className={classes.title}>{title}</h1>
            {badges}
          </div>
          {description && <p className={classes.description}>{description}</p>}
        </div>
        {actions && <div className={classes.actions}>{actions}</div>}
      </div>
      <div className={classes.content}>{children}</div>
    </div>
  );
};

export default Page;
