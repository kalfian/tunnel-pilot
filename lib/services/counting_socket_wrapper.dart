import 'dart:async';
import 'dart:io';

class CountingSocketWrapper {
  int bytesUp = 0;
  int bytesDown = 0;

  StreamSubscription<List<int>>? _channelSub;
  StreamSubscription<List<int>>? _localSub;

  StreamSubscription<List<int>> pipeChannelToLocal(
    Stream<List<int>> channelStream,
    Socket localSocket,
  ) {
    _channelSub = channelStream.listen(
      (data) {
        bytesDown += data.length;
        localSocket.add(data);
      },
      onError: (_) => localSocket.destroy(),
      onDone: () => localSocket.close(),
      cancelOnError: true,
    );
    return _channelSub!;
  }

  StreamSubscription pipeLocalToChannel(
    Socket localSocket,
    StreamSink<List<int>> channelSink,
  ) {
    _localSub = localSocket.listen(
      (data) {
        bytesUp += data.length;
        channelSink.add(data);
      },
      onError: (_) => channelSink.close(),
      onDone: () => channelSink.close(),
      cancelOnError: true,
    );
    return _localSub!;
  }

  void dispose() {
    _channelSub?.cancel();
    _localSub?.cancel();
  }
}
