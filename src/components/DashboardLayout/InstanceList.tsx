import { faServer } from '@fortawesome/free-solid-svg-icons';
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome';
import { RadioGroup } from '@headlessui/react';
import InstanceCard from 'components/InstanceCard';
import InstanceLoadingCard from 'components/InstanceLoadingCard';
import InstancePill from 'components/InstancePill';
import { InstanceContext } from 'data/InstanceContext';
import { NotificationContext } from 'data/NotificationContext';
import { useUserLoggedIn } from 'data/UserInfo';
import { Fragment, useContext, useEffect } from 'react';
import { useEffectOnce } from 'usehooks-ts';
import useAnalyticsEventTracker from 'utils/hooks';
import { match, otherwise } from 'variant';

export default function InstanceList({
  className = '',
  children,
}: {
  className?: string;
  children?: React.ReactNode;
}) {
  const gaEventTracker = useAnalyticsEventTracker('Instance List');
  const {
    instanceList: instances,
    selectedInstance,
    selectInstance,
    isReady,
  } = useContext(InstanceContext);
  const { ongoingNotifications } = useContext(NotificationContext);
  const userLoggedIn = useUserLoggedIn();

  useEffect(() => {
    if (!isReady) return;
    gaEventTracker(
      'View',
      'Instance List',
      true,
      Object.keys(instances).length
    );
  }, [isReady, instances]);

  return (
    <RadioGroup
      className={`gap flex min-h-0 flex-col gap-y-1 px-1 -mx-1 overflow-y-auto child:w-full ${className}`}
      value={selectedInstance}
      onChange={selectInstance}
    >
      <RadioGroup.Label className="sr-only">Instances</RadioGroup.Label>
      {userLoggedIn ? (
        instances &&
        Object.values(instances).map((instance) => (
          <RadioGroup.Option
            key={instance.uuid}
            value={instance}
            className="outline-none child:w-full"
          >
            <InstancePill {...instance} />
          </RadioGroup.Option>
        ))
      ) : (
        <div
          className={`flex w-fit select-none flex-col items-stretch gap-4 rounded-xl border border-gray-faded/30 bg-gray-800 py-4 px-6 text-medium font-bold tracking-tight`}
        >
          <FontAwesomeIcon icon={faServer} className="text-h1 text-gray-400" />
          <p className="text-xl text-center text-gray-400">
            Log in to view game server instances.
          </p>
        </div>
      )}
      {ongoingNotifications &&
        ongoingNotifications
          .map((notification) => {
            if (!notification.start_value) return null;
            if (notification.state === 'done') return null;
            return match(
              notification.start_value,
              otherwise(
                {
                  InstanceCreation: ({
                    instance_uuid,
                    instance_name,
                    port,
                    flavour,
                    game_type,
                  }) => (
                    <InstanceLoadingCard
                      key={instance_uuid}
                      uuid={instance_uuid}
                      name={instance_name}
                      port={port}
                      flavour={flavour}
                      game_type={game_type}
                      level={notification.level}
                      state={notification.state}
                      progress_percent={
                        notification.total
                          ? notification.progress / notification.total
                          : undefined
                      }
                      progress_title={'Setting up...'}
                    />
                  ),
                },
                (_) => null
              )
            );
          })
          .reverse()}
      {children}
    </RadioGroup>
  );
}
