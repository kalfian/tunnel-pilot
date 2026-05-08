import 'dart:async';
import 'dart:io';

class CountingSocketWrapper {
  int bytesUp = 0;
  int bytesDown = 0;

  StreamSubscription<List<int>> pipeChannelToLocal(
    Stream<List<int>> channelStream,
    Socket localSocket,
  ) {
    return channelStream.listen(
      (data) {
        bytesDown += data.length;
        localSocket.add(data);
      },
      onError: (_) => localSocket.destroy(),
      onDone: () => localSocket.close(),
    );
  }

  StreamSubscription pipeLocalToChannel(
    Socket localSocket,
    StreamSink<List<int>> channelSink,
  ) {
    return localSocket.listen(
      (data) {
        bytesUp += data.length;
        channelSink.add(data);
      },
      onError: (_) => channelSink.close(),
      onDone: () => channelSink.close(),
    );
  }
}
